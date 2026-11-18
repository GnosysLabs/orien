//! The website's chat: the same expert, answering visitors of orien.network
//! from its chat bubble. It listens on this computer only; nginx passes it
//! the site's /api/ask. Questions and answers are not kept or written down.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Result;
use axum::Json;
use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use serde::Deserialize;

use crate::expert::{Expert, Turn};

/// Most turns of a chat the model reads, and most characters of each.
const MAX_TURNS: usize = 12;
const MAX_CHARS: usize = 1500;

struct Limits {
    per_visitor: usize,
    per_day: usize,
    everyone: VecDeque<Instant>,
    visitors: HashMap<String, VecDeque<Instant>>,
}

impl Limits {
    /// Whether `visitor` may ask now, counting the question if so.
    fn allow(&mut self, visitor: &str) -> Result<(), &'static str> {
        let now = Instant::now();
        let hour = Duration::from_secs(3600);
        let day = Duration::from_secs(86_400);
        while self.everyone.front().is_some_and(|at| now - *at > day) {
            self.everyone.pop_front();
        }
        self.visitors.retain(|_, asked| {
            while asked.front().is_some_and(|at| now - *at > hour) {
                asked.pop_front();
            }
            !asked.is_empty()
        });
        if self.everyone.len() >= self.per_day {
            return Err("The assistant has answered all it can for today. Please try again tomorrow, or ask in Orien's Discord.");
        }
        let asked = self.visitors.entry(visitor.to_string()).or_default();
        if asked.len() >= self.per_visitor {
            return Err("You've asked a lot of questions in a short time. Please try again in a little while.");
        }
        asked.push_back(now);
        self.everyone.push_back(now);
        Ok(())
    }
}

struct Web {
    expert: Arc<Expert>,
    limits: Mutex<Limits>,
}

#[derive(Deserialize)]
struct Ask {
    messages: Vec<Turn>,
}

fn refuse(status: StatusCode, error: &str) -> Response {
    (status, Json(serde_json::json!({ "error": error }))).into_response()
}

/// The visitor's address: Cloudflare names it, and nginx passes that on.
fn visitor(headers: &HeaderMap) -> String {
    let named = |name: &str| headers.get(name).and_then(|v| v.to_str().ok()).map(|v| v.split(',').next().unwrap_or("").trim().to_string());
    named("cf-connecting-ip").or_else(|| named("x-forwarded-for")).unwrap_or_default()
}

async fn ask(State(web): State<Arc<Web>>, headers: HeaderMap, body: axum::body::Bytes) -> Response {
    let Ok(Ask { messages }) = serde_json::from_slice::<Ask>(&body) else {
        return refuse(StatusCode::BAD_REQUEST, "Bad request.");
    };
    let turns: Vec<Turn> = messages
        .into_iter()
        .filter(|turn| turn.role == "user" || turn.role == "assistant")
        .map(|turn| Turn { role: turn.role, content: turn.content.chars().take(MAX_CHARS).collect() })
        .collect();
    let turns: Vec<Turn> = turns[turns.len().saturating_sub(MAX_TURNS)..].to_vec();
    if turns.last().is_none_or(|last| last.role != "user" || last.content.trim().is_empty()) {
        return refuse(StatusCode::BAD_REQUEST, "Ask a question to get started.");
    }
    if let Err(why) = web.limits.lock().unwrap().allow(&visitor(&headers)) {
        return refuse(StatusCode::TOO_MANY_REQUESTS, why);
    }

    let (out, pieces) = tokio::sync::mpsc::channel::<String>(64);
    let expert = web.expert.clone();
    tokio::spawn(async move {
        let started = Instant::now();
        match expert.web_answer(&turns, out.clone()).await {
            Ok(()) => eprintln!("website: answered in {:.1}s", started.elapsed().as_secs_f32()),
            Err(error) => {
                eprintln!("website: could not answer: {error:#}");
                let _ = out.send("Sorry, I couldn't answer just now. Please try again in a moment.".to_string()).await;
            }
        }
    });
    let stream = tokio_stream::wrappers::ReceiverStream::new(pieces).map(Ok::<_, std::convert::Infallible>);
    Response::builder()
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        // Words appear as they are written: nothing in between holds them back.
        .header(header::CACHE_CONTROL, "no-store, no-transform")
        .header("X-Accel-Buffering", "no")
        .body(Body::from_stream(stream))
        .unwrap_or_else(|_| refuse(StatusCode::INTERNAL_SERVER_ERROR, "Something went wrong."))
}

use tokio_stream::StreamExt as _;

/// Answers the website's chat at `listen` for as long as the bot runs.
pub async fn serve(expert: Arc<Expert>, listen: String, per_visitor: usize, per_day: usize) -> Result<()> {
    let web = Arc::new(Web {
        expert,
        limits: Mutex::new(Limits { per_visitor, per_day, everyone: VecDeque::new(), visitors: HashMap::new() }),
    });
    let app = axum::Router::new().route("/api/ask", post(ask)).with_state(web);
    let listener = tokio::net::TcpListener::bind(&listen).await?;
    eprintln!("website chat listening on {listen}");
    axum::serve(listener, app).await?;
    Ok(())
}
