import{_ as e,a as t,c as n,d as r,f as i,g as a,h as o,i as s,l as c,m as l,n as u,o as d,p as f,r as p,s as m,t as h,u as g}from"./vendor-DRVLJQrl.js";(function(){let e=document.createElement(`link`).relList;if(e&&e.supports&&e.supports(`modulepreload`))return;for(let e of document.querySelectorAll(`link[rel="modulepreload"]`))n(e);new MutationObserver(e=>{for(let t of e)if(t.type===`childList`)for(let e of t.addedNodes)e.tagName===`LINK`&&e.rel===`modulepreload`&&n(e)}).observe(document,{childList:!0,subtree:!0});function t(e){let t={};return e.integrity&&(t.integrity=e.integrity),e.referrerPolicy&&(t.referrerPolicy=e.referrerPolicy),t.credentials=e.crossOrigin===`use-credentials`?`include`:e.crossOrigin===`anonymous`?`omit`:`same-origin`,t}function n(e){if(e.ep)return;e.ep=!0;let n=t(e);fetch(e.href,n)}})();var _=typeof window<`u`&&window.__NETVIZ__||{},v=(_.api||`/api`).replace(/\/$/,``),y=(_.assets||`/vendor/`).replace(/\/?$/,`/`),b=!!_.readOnly,x=!!_.embed,S=typeof window<`u`?new URLSearchParams(window.location.search).get(`view`):null,C=S===`details`||S===`simple`?S:null,w=()=>document.getElementById(`app`);async function T(e,t={}){let n=await fetch(v+e,{headers:{"Content-Type":`application/json`,...t.headers||{}},...t}),r=await n.text(),i={};try{i=r?JSON.parse(r):{}}catch{i={raw:r}}if(!n.ok){let e=i.error||`HTTP ${n.status}`;throw Error(e)}return i}function ee(){return T(`/state`)}function te(){return T(`/network`)}function ne(){return T(`/progress`)}function E(e=null,t=!1){let n=new URLSearchParams;e&&n.set(`server`,e),t&&n.set(`force`,`1`);let r=n.toString();return T(`/bgp${r?`?${r}`:``}`)}function D(e){return T(`/measurement/${e}`)}function re(){return T(`/refresh`,{method:`POST`,body:`{}`})}function ie(e){return T(`/settings`,{method:`POST`,body:JSON.stringify({refresh_interval_minutes:e})})}var O=`#7e889b`,k={good:{color:`#4ade80`,bg:`rgba(74, 222, 128, 0.14)`,border:`rgba(74, 222, 128, 0.42)`,label:`OK`,longLabel:`Healthy`,icon:`●`,severity:0},warning:{color:`#fbbf24`,bg:`rgba(251, 191, 36, 0.15)`,border:`rgba(251, 191, 36, 0.45)`,label:`BLIP`,longLabel:`Brief blip`,icon:`▲`,severity:1},serious:{color:`#fb923c`,bg:`rgba(251, 146, 60, 0.18)`,border:`rgba(251, 146, 60, 0.5)`,label:`LOSS`,longLabel:`Lasting loss`,icon:`◆`,severity:2},critical:{color:`#f87171`,bg:`rgba(248, 113, 113, 0.18)`,border:`rgba(248, 113, 113, 0.55)`,label:`NO REPLY`,longLabel:`Not answering`,icon:`✕`,severity:3},error:{color:`#8f96a6`,bg:`rgba(143, 150, 166, 0.14)`,border:`rgba(143, 150, 166, 0.35)`,label:`ERR`,longLabel:`Probe Error`,icon:`!`,severity:1},unknown:{color:`#6b7280`,bg:`rgba(107, 114, 128, 0.14)`,border:`rgba(107, 114, 128, 0.3)`,label:`WAIT`,longLabel:`Pending`,icon:`○`,severity:-1}};function A(e){return k[e]||k.unknown}function j(e,t={}){return e?t[String(e)]||{asn:e,name:`AS${e}`,holder:`AS${e}`,color:O}:{asn:null,name:`Unknown`,holder:`Unknown`,color:`#6b7280`}}function M(e,t=1){if(!e||!e.startsWith(`#`))return`rgba(126,136,155,${t})`;let n=e.slice(1);return`rgba(${parseInt(n.slice(0,2),16)}, ${parseInt(n.slice(2,4),16)}, ${parseInt(n.slice(4,6),16)}, ${t})`}function N(e){return e==null?``:String(e).replace(/&/g,`&amp;`).replace(/</g,`&lt;`).replace(/>/g,`&gt;`).replace(/"/g,`&quot;`).replace(/'/g,`&#39;`)}function P(e,t=!1){if(!e)return`—`;let n=new Date(e.endsWith(`Z`)||e.includes(`+`)?e:`${e}Z`);if(Number.isNaN(n.getTime()))return e;let r=String(n.getUTCHours()).padStart(2,`0`),i=String(n.getUTCMinutes()).padStart(2,`0`),a=String(n.getUTCSeconds()).padStart(2,`0`);return t?`${n.toLocaleString(`en-US`,{month:`short`,timeZone:`UTC`})} ${String(n.getUTCDate()).padStart(2,`0`)} ${r}:${i} UTC`:`${r}:${i}:${a} UTC`}function F(e){if(e==null||e<0)return`—`;let t=Math.floor(e),n=Math.floor(t/60),r=t%60;return n>=60?`${Math.floor(n/60)}h ${n%60}m`:`${String(n).padStart(2,`0`)}:${String(r).padStart(2,`0`)}`}var I={lat:44,lng:-54,altitude:2.42},ae=185,oe=1100,se=4.4,ce=3e4,L={lat:38,lng:-50},le=9,R=.42,z={Lauterbourg:`Europe`};function B(e,t,n){return Math.max(t,Math.min(n,e))}function ue(e){return e<.5?4*e*e*e:1-(-2*e+2)**3/2}function V(e,t,n,r){let i=Math.PI/180,a=(n-e)*i,o=(r-t)*i,s=Math.sin(a/2)**2+Math.cos(e*i)*Math.cos(n*i)*Math.sin(o/2)**2;return 2*Math.asin(Math.min(1,Math.sqrt(s)))/i}function H(e,t){let n=(t-e+540)%360-180;return n<-180&&(n+=360),n}function U(){let e=document.createElement(`canvas`);e.width=64,e.height=64;let t=e.getContext(`2d`),n=t.createRadialGradient(32,32,0,32,32,32);n.addColorStop(0,`rgba(255,255,255,1)`),n.addColorStop(.25,`rgba(210,228,255,0.85)`),n.addColorStop(.6,`rgba(130,170,255,0.2)`),n.addColorStop(1,`rgba(0,0,0,0)`),t.fillStyle=n,t.fillRect(0,0,64,64);let r=new d(e);return r.needsUpdate=!0,r}var de=class{constructor(e,t={}){this.container=e,this.callbacks=t,this.reducedMotion=window.matchMedia(`(prefers-reduced-motion: reduce)`).matches,this.isHidden=document.hidden,this.servers=[],this.serversById={},this.pairData={},this.carriers={},this.routeMode=!1,this.hasIncident=!1,this.selectedServerId=null,this.selectedPairKey=null,this.selectedPairMeasurement=null,this.hoveredPairKey=null,this.hoveredHopIndex=null,this._idleTimer=null,this._flightRaf=null,this._fxRaf=null,this._currentAltitude=I.altitude,this._labelNodes=new Map,this._hopMeshes=new Map,this._arcMeshesLossy=[],this._starfield=null,this._lastZoomAltitude=I.altitude,this._arcsKept=new Map,this._ringsKept=new Map,this.simpleMode=!1,this.validatorInfo={},this._simpleCenter=null,this._simpleAltitude=4,this._lastSwayOffset=0,this._initGlobe(),this._initStarfield(),this._loadCountryBorders(),this._bindLifecycle(),this._startEffectsLoop()}_initGlobe(){let e=this.container.clientWidth||window.innerWidth,t=this.container.clientHeight||window.innerHeight;this.world=new u(this.container,{animateIn:!1,rendererConfig:{antialias:!0,alpha:!0,powerPreference:`high-performance`}}).width(e).height(t).backgroundColor(`rgba(0, 0, 0, 0)`).globeImageUrl(`${y}earth-night-orien.jpg`).bumpImageUrl(`${y}earth-topology.png`).showAtmosphere(!0).atmosphereColor(`#d3c4ff`).atmosphereAltitude(.21).lineHoverPrecision(2.2).showPointerCursor(e=>[`point`,`arc`,`object`].includes(e));let r=this.world.renderer();r.setPixelRatio(Math.min(window.devicePixelRatio||1,2)),r.toneMapping=4,r.toneMappingExposure=1.15;let i=this.world.globeMaterial();i&&(i.bumpScale=2.6,i.shininess=10,i.specular&&=new m(`#1c1c26`),i.emissive&&(i.emissive=new m(`#0b0b0f`),i.emissiveIntensity=.35));let a=new p(`#ececf1`,1.35),o=new n(`#ffffff`,1.25);o.position.set(160,120,220);let s=new n(`#d3c4ff`,.55);s.position.set(-200,-60,-140),this.world.lights([a,o,s]);let c=this.world.controls();c.enableDamping=!0,c.dampingFactor=.08,c.enablePan=!1,c.minDistance=125,c.maxDistance=470,c.rotateSpeed=.65,c.zoomSpeed=.85,c.autoRotate=!this.reducedMotion,c.autoRotateSpeed=.14,x&&(c.enabled=!1),c.addEventListener(`start`,()=>{this._cancelFlight(),this._pauseAutoRotate()}),c.addEventListener(`end`,()=>{this._scheduleIdleAutoRotate()}),this.world.pointLat(`lat`).pointLng(`lon`).pointAltitude(e=>e.dimmed?.008:e.selected?.045:.024).pointRadius(e=>e.dimmed?.28:e.selected?.65:.48).pointColor(e=>e.beaconColor).pointResolution(18).pointsTransitionDuration(250).pointLabel(e=>this._buildServerTooltip(e)).onPointClick(e=>{e&&e.id&&this.callbacks.onServerClick&&this.callbacks.onServerClick(e.id)}),this.world.ringLat(`lat`).ringLng(`lon`).ringAltitude(.006).ringColor(e=>t=>M(e.ringHex,Math.max(0,(1-t)*e.ringAlpha))).ringMaxRadius(e=>e.maxRadius).ringPropagationSpeed(e=>this.reducedMotion?0:e.propagationSpeed).ringRepeatPeriod(e=>this.reducedMotion?6e4:e.repeatPeriod).ringResolution(48),this.world.htmlLat(`lat`).htmlLng(`lon`).htmlAltitude(.03).htmlTransitionDuration(0).htmlElement(e=>this._createOrUpdateServerLabel(e)),this.world.arcStartLat(`startLat`).arcStartLng(`startLng`).arcEndLat(`endLat`).arcEndLng(`endLng`).arcAltitude(`altitude`).arcColor(`color`).arcStroke(`stroke`).arcDashLength(e=>this.reducedMotion?1:e.dashLength).arcDashGap(e=>this.reducedMotion?0:e.dashGap).arcDashInitialGap(`dashInitialGap`).arcDashAnimateTime(e=>this.reducedMotion||e.pairKey&&e.pairKey===this.hoveredPairKey?0:e.animateTime).arcCurveResolution(48).arcCircularResolution(6).arcsTransitionDuration(260).arcLabel(e=>this._buildArcTooltip(e)).onArcClick(e=>{this.simpleMode||e&&e.pairKey&&this.callbacks.onArcClick&&this.callbacks.onArcClick(e.pairKey)}).onArcHover(e=>{let t=e?e.pairKey:null;t!==this.hoveredPairKey&&(this.hoveredPairKey=t,this._holdStill(!!t),this.callbacks.onArcHover&&this.callbacks.onArcHover(t),this._refreshArcsVisualState())}),this.world.objectLat(`lat`).objectLng(`lon`).objectAltitude(`alt`).objectFacesSurface(!0).objectThreeObject(e=>this._createHopMesh(e)).objectLabel(e=>this._buildHopTooltip(e)).onObjectHover(e=>{let t=e?e.hop:null,n=e?e.pairKey:null;this.callbacks.onHopHover&&this.callbacks.onHopHover(t,n),this.setHoveredHop(t,n)}).onObjectClick(e=>{e&&e.pairKey&&this.callbacks.onArcClick&&this.callbacks.onArcClick(e.pairKey,e.hop)}),this.world.onZoom(e=>{e&&typeof e.altitude==`number`&&(this._currentAltitude=e.altitude,Math.abs(e.altitude-this._lastZoomAltitude)>.03&&(this._lastZoomAltitude=e.altitude,this._updateLabelVisibilityByZoom()))}),this.world.pointOfView(I,0)}_initStarfield(){let e=1700,n=new Float32Array(e*3),r=new Float32Array(e*3),i=[new m(`#ececf1`),new m(`#ffffff`),new m(`#d3c4ff`),new m(`#ffe9d2`)];for(let t=0;t<e;t++){let e=Math.random(),a=Math.random(),o=2*Math.PI*e,s=Math.acos(2*a-1),c=480+Math.random()*280;n[t*3]=c*Math.sin(s)*Math.cos(o),n[t*3+1]=c*Math.sin(s)*Math.sin(o),n[t*3+2]=c*Math.cos(s);let l=i[Math.floor(Math.random()*i.length)],u=.45+Math.random()*.55;r[t*3]=l.r*u,r[t*3+1]=l.g*u,r[t*3+2]=l.b*u}let a=new t;a.setAttribute(`position`,new s(n,3)),a.setAttribute(`color`,new s(r,3));let o=new l({size:3.1,map:U(),vertexColors:!0,transparent:!0,opacity:.72,depthWrite:!1,blending:2});this._starfield=new f(a,o),this._starfield.renderOrder=-1,this.world.scene().add(this._starfield)}async _loadCountryBorders(){try{let e=await fetch(`${y}countries-110m.json`);if(!e.ok)return;let t=await e.json(),n=h(t,t.objects.countries).features;this.world.polygonsData(n).polygonAltitude(.003).polygonCapColor(()=>`rgba(10, 22, 42, 0.06)`).polygonSideColor(()=>`rgba(0, 0, 0, 0)`).polygonStrokeColor(()=>`rgba(211, 196, 255, 0.2)`).polygonsTransitionDuration(0)}catch(e){console.warn(`Could not load country borders:`,e)}}_bindLifecycle(){this._onResize=()=>{let e=this.container.clientWidth||window.innerWidth,t=this.container.clientHeight||window.innerHeight;this.world.width(e).height(t),this.world.renderer().setPixelRatio(Math.min(window.devicePixelRatio||1,2)),this.simpleMode||this._placeDetailsGlobe(),this.callbacks.onResize&&this.callbacks.onResize()},window.addEventListener(`resize`,this._onResize),typeof ResizeObserver<`u`&&(this._resizeObserver=new ResizeObserver(()=>this._onResize()),this._resizeObserver.observe(this.container)),this._onVisibility=()=>{this.isHidden=document.hidden,this.isHidden?(this.world.pauseAnimation(),this._fxRaf&&=(cancelAnimationFrame(this._fxRaf),null)):(this.world.resumeAnimation(),this._startEffectsLoop())},document.addEventListener(`visibilitychange`,this._onVisibility);let e=window.matchMedia(`(prefers-reduced-motion: reduce)`);this._onMotionChange=e=>{this.reducedMotion=e.matches;let t=this.world.controls();this.reducedMotion&&(t.autoRotate=!1),this.render()},e.addEventListener(`change`,this._onMotionChange)}_holdStill(e){let t=this.world.controls();e&&!this._still?(this._wasTurning=t.autoRotate,t.autoRotate=!1):!e&&this._still&&this._wasTurning&&!this.reducedMotion&&!this.isHidden&&(t.autoRotate=!0),this._still=e}_pauseAutoRotate(){this._idleTimer&&=(clearTimeout(this._idleTimer),null);let e=this.world.controls();e.autoRotate=!1}_scheduleIdleAutoRotate(){this._idleTimer&&=(clearTimeout(this._idleTimer),null),this.reducedMotion||this.selectedServerId||this.selectedPairKey||(this._idleTimer=setTimeout(()=>{this._still?this._wasTurning=!0:this.selectedServerId||this.selectedPairKey||this.reducedMotion||this.isHidden||(this.world.controls().autoRotate=!0)},this.simpleMode?4e3:ce))}_cancelFlight(){this._flightRaf&&=(cancelAnimationFrame(this._flightRaf),null)}flyTo(e,t=1450){if(this._cancelFlight(),this._pauseAutoRotate(),this.reducedMotion||t<=0){this.world.pointOfView(e,0),this._currentAltitude=e.altitude,this._updateLabelVisibilityByZoom();return}let n=this.world.pointOfView(),r=Number.isFinite(n.lat)?n.lat:I.lat,i=Number.isFinite(n.lng)?n.lng:I.lng,a=Number.isFinite(n.altitude)?n.altitude:I.altitude,o=e.lat-r,s=H(i,e.lng),c=e.altitude-a,l=Math.hypot(o,s),u=performance.now(),d=e=>{let n=B((e-u)/t,0,1),f=ue(n),p=l>35?Math.sin(Math.PI*n)*Math.min(.24,l*.0025):0,m=c<-.15?-.055*Math.sin(Math.PI*n**1.35):0,h=r+o*f,g=i+s*f,_=B(a+c*f+p+m,.45,3.2);this.world.pointOfView({lat:h,lng:g,altitude:_},0),this._currentAltitude=_,n<1?this._flightRaf=requestAnimationFrame(d):(this._flightRaf=null,this._updateLabelVisibilityByZoom())};this._flightRaf=requestAnimationFrame(d)}flyToServer(e){let t=this.serversById[e];t&&this.flyTo({lat:t.lat-1.2,lng:t.lon+9.5,altitude:1.02},1450)}flyToPair(e,t,n=[]){let r=this.serversById[e],i=this.serversById[t];if(!r||!i)return;let a=[{lat:r.lat,lon:r.lon},{lat:i.lat,lon:i.lon}];if(Array.isArray(n))for(let e of n)typeof e.lat==`number`&&typeof e.lon==`number`&&a.push({lat:e.lat,lon:e.lon});let o=0,s=0,c=0;for(let e of a){let t=e.lat*Math.PI/180,n=e.lon*Math.PI/180;o+=Math.cos(t)*Math.cos(n),s+=Math.cos(t)*Math.sin(n),c+=Math.sin(t)}let l=Math.atan2(c,Math.hypot(o,s))*180/Math.PI,u=Math.atan2(s,o)*180/Math.PI,d=V(r.lat,r.lon,i.lat,i.lon);for(let e of a)d=Math.max(d,V(l,u,e.lat,e.lon)*2);let f=B(1.06+d/42,1.24,2.28),p=B(14+d*.22,14,30);this.flyTo({lat:l-4.5,lng:u+p,altitude:f},1450)}setSimpleMode(e,t=null){let n=this.simpleMode;this.simpleMode=!!e;let r=this.world.controls();this.world.camera(),this.simpleMode?(r.enableZoom=!1,r.maxDistance=1400,r.autoRotateSpeed=R/6,this._starfield&&(this._starfield.scale.setScalar(3),this._starfield.material.opacity=.2,this._starfield.material.size=5.2),t&&this.setSimpleLayout(t,!n),this.reducedMotion||(r.autoRotate=!0)):(r.enableZoom=!0,r.maxDistance=470,r.autoRotateSpeed=.14,this._placeDetailsGlobe(),this._simpleCenter=null,this._starfield&&(this._starfield.scale.setScalar(1),this._starfield.material.opacity=.72,this._starfield.material.size=3.1),n&&this.world.pointOfView(I,0)),this.render()}_placeDetailsGlobe(){let e=this.container.clientWidth||window.innerWidth;this.world.globeOffset([e>=oe?ae:0,0])}setSimpleLayout({cx:e,cy:t,globePx:n},r=!1){let i=this.world.camera(),a=this.container.clientWidth||window.innerWidth,o=this.container.clientHeight||window.innerHeight,s=this.world.getGlobeRadius(),c=g.degToRad(i.fov/2),l=Math.atan(n/(o/2)*Math.tan(c)),u=s/Math.sin(l);this._simpleAltitude=u/s-1,this._simpleCenter={cx:e,cy:t},this.world.globeOffset([e-a/2,t-o/2]);let d=this.world.pointOfView(),f=!r&&this.simpleMode;this.world.pointOfView({lat:f?d.lat:L.lat,lng:f?d.lng:L.lng,altitude:this._simpleAltitude},0),this._currentAltitude=this._simpleAltitude}getGlobeCircle(){let e=this.world.camera(),t=this.container.clientWidth||window.innerWidth,n=this.container.clientHeight||window.innerHeight,r=this.world.getGlobeRadius(),i=e.position.length(),a=g.degToRad(e.fov/2),o=i>r?n/2*Math.tan(Math.asin(r/i))/Math.tan(a):n,s=this._simpleCenter||{cx:t/2,cy:n/2};return{cx:s.cx,cy:s.cy,r:o}}setValidators(e){this.validatorInfo=e||{},this.render()}_swayTick(){let e=this.world.controls();if(!e.autoRotate)return;let t=this.world.pointOfView(),n=H(L.lng,t.lng),r=Math.abs(n)>le,i=(n-this._lastSwayOffset)*Math.sign(n)>0;r&&i&&(e.autoRotateSpeed=-e.autoRotateSpeed),this._lastSwayOffset=n}resetView(){this.selectedServerId=null,this.selectedPairKey=null,this.selectedPairMeasurement=null,this.hoveredHopIndex=null,this.render(),this.simpleMode?this.reducedMotion||(this.world.controls().autoRotate=!0):(this.flyTo(I,1300),this.reducedMotion||setTimeout(()=>{this.selectedServerId||this.selectedPairKey||this.reducedMotion||this.isHidden||(this.world.controls().autoRotate=!0)},1450))}setData({servers:e,pairData:t,carriers:n,routeMode:r,hasIncident:i,isMeasuring:a}){e&&(this.servers=e,this.serversById=Object.fromEntries(e.map(e=>[e.id,e]))),t!==void 0&&(this.pairData=t),n!==void 0&&(this.carriers=n),r!==void 0&&(this.routeMode=r),i!==void 0&&(this.hasIncident=i),a&&!this.simpleMode?this._pauseAutoRotate():this.selectedServerId||this.selectedPairKey||this.reducedMotion||this._idleTimer||this._scheduleIdleAutoRotate(),this.world.atmosphereColor(`#d3c4ff`).atmosphereAltitude(.21),this.render()}setSelection({serverId:e=null,pairKey:t=null,measurement:n=null}){this.selectedServerId=e,this.selectedPairKey=t,this.selectedPairMeasurement=n,this.hoveredHopIndex=null,(e||t)&&this._pauseAutoRotate(),this.render()}setRouteMode(e){this.routeMode=!!e,this.render()}setHoveredPair(e){this.hoveredPairKey!==e&&(this.hoveredPairKey=e,this._refreshArcsVisualState())}setHoveredHop(e,t=null){this.hoveredHopIndex=e;let n=t||this.selectedPairKey;for(let[t,r]of this._hopMeshes.entries()){let t=e!==null&&r.hop===e&&(!n||r.pairKey===n);r.isHovered=t;let i=t?2.1:r.isCulprit?1.35:1;r.group.scale.setScalar(i),r.halo&&(r.halo.visible=t||r.isCulprit)}}_computeServerHealth(e){let t=`unknown`,n=`unknown`,r=0,i=0;for(let a of Object.values(this.pairData||{})){if(!a||a.status===`error`)continue;let o=A(a.status).severity;a.dst===e&&(i+=1,(a.dest_loss||0)>r&&(r=a.dest_loss),o>A(t).severity&&(t=a.status)),(a.src===e||a.dst===e)&&o>A(n).severity&&(n=a.status)}let a=this.validatorInfo[e];return a&&a.up===!1?{status:`critical`,hasData:!0,maxInboundLoss:r}:i===0&&n===`unknown`?{status:`good`,hasData:!1,maxInboundLoss:0}:t===`critical`?{status:`critical`,hasData:!0,maxInboundLoss:r}:n===`serious`||n===`warning`||n===`critical`?{status:n===`critical`?`serious`:n,hasData:!0,maxInboundLoss:r}:{status:`good`,hasData:!0,maxInboundLoss:r}}_simpleServerHealth(e){let t=this.validatorInfo[e];return{status:t&&t.up===!1?`critical`:`good`,hasData:!!t,maxInboundLoss:0}}render(){this.servers.length&&(this._renderServersAndRings(),this._renderArcs(),this._renderHopMarkers())}_renderServersAndRings(){let e=!!(this.selectedServerId||this.selectedPairKey),t=this.selectedPairKey?this.selectedPairKey.split(`->`)[0]:null,n=this.selectedPairKey?this.selectedPairKey.split(`->`)[1]:null,r=[],i=[],a=[];for(let o of this.servers){let s=this.simpleMode?this._simpleServerHealth(o.id):this._computeServerHealth(o.id),c=A(s.status),l=this.selectedServerId===o.id||t===o.id||n===o.id,u=e&&!l;r.push({...o,healthStatus:s.status,hasData:s.hasData,maxInboundLoss:s.maxInboundLoss,selected:l,dimmed:u,beaconColor:u?M(c.color,.25):c.color});let d=1.35,f=1950,p=3.9;s.status===`critical`?(d=4.6,f=650,p=6.4):(s.status===`serious`||s.status===`warning`)&&(d=2.7,f=1120,p=4.9),i.push({id:o.id,lat:o.lat,lon:o.lon,ringHex:c.color,ringAlpha:u?.14:l?.95:.78,propagationSpeed:d,repeatPeriod:f,maxRadius:l?p*1.15:p}),a.push({...o,healthStatus:s.status,hasData:s.hasData,maxInboundLoss:s.maxInboundLoss,selected:l,dimmed:u})}this.world.pointsData(r),this.world.ringsData(this._keepSame(this._ringsKept,i,e=>String(e.id))),this.world.htmlElementsData(a),this._updateLabelVisibilityByZoom()}_createOrUpdateServerLabel(e){let t=A(e.healthStatus),n=document.createElement(`button`);if(n.type=`button`,n.className=`globe-server-label offset-${e.id}${e.selected?` is-selected`:``}${e.dimmed?` is-dimmed`:``}`,n.setAttribute(`aria-label`,`${e.name} (${e.city}, ${e.country}) - ${t.longLabel}`),this.simpleMode){let r=document.createElement(`div`);return r.className=`gsl-anchor`,n.className=`globe-server-label is-simple place-${e.id}`,n.innerHTML=`
        <span class="gsl-dot" style="background:${t.color};box-shadow:0 0 8px ${t.color}"></span>
        <span class="gsl-place">${N(z[e.city]||e.city)}</span>
      `,n.addEventListener(`click`,t=>{t.stopPropagation(),this.callbacks.onServerClick&&this.callbacks.onServerClick(e.id)}),r.appendChild(n),this._labelNodes.set(e.id,n),r}return n.innerHTML=`
      <span class="gsl-dot" style="background:${t.color};box-shadow:0 0 8px ${t.color}"></span>
      <span class="gsl-name">${N(e.name)}</span>
      <span class="gsl-city">${N(e.city)}</span>
      ${e.maxInboundLoss>0?`<span class="gsl-loss" style="color:${t.color}">${e.maxInboundLoss.toFixed(0)}%</span>`:``}
    `,n.addEventListener(`click`,t=>{t.stopPropagation(),this.callbacks.onServerClick&&this.callbacks.onServerClick(e.id)}),this._labelNodes.set(e.id,n),this._applyZoomClassToLabel(n,e),n}_applyZoomClassToLabel(e,t){if(!e||this.simpleMode)return;let n=this._currentAltitude,r=n<1.95||t&&t.selected,i=n>2.45&&!(t&&t.selected);e.classList.toggle(`is-close`,r),e.classList.toggle(`is-far`,i)}_updateLabelVisibilityByZoom(){for(let[e,t]of this._labelNodes.entries()){let n=this.selectedServerId===e||this.selectedPairKey&&this.selectedPairKey.split(`->`).includes(e);this._applyZoomClassToLabel(t,{selected:n})}}_buildSimpleArcs(){let e=[];for(let t=0;t<this.servers.length;t++)for(let n=t+1;n<this.servers.length;n++){let r=this.servers[t],i=this.servers[n],a=[this.pairData[`${r.id}->${i.id}`],this.pairData[`${i.id}->${r.id}`]].filter(e=>e&&e.status&&e.status!==`error`&&e.status!==`unknown`),o=`unknown`,s=0,c=0,l=0;for(let e of a)A(e.status).severity>A(o).severity&&(o=e.status),typeof e.dest_loss==`number`&&(s=Math.max(s,e.dest_loss)),typeof e.dest_avg_rtt==`number`&&(c+=e.dest_avg_rtt,l+=1);let u=o!==`unknown`,d=u?A(o).color:`#9fb4d8`,f=u?.9:.45,p=V(r.lat,r.lon,i.lat,i.lon),m={pairKey:`${r.id}->${i.id}`,srcId:r.id,dstId:i.id,startLat:r.lat,startLng:r.lon,endLat:i.lat,endLng:i.lon,altitude:B(.03+p/520,.05,.2),baseHex:d,status:o,loss:s,rtt:l?c/l:null,fromCity:r.city,toCity:i.city,isSimple:!0,isLossy:!1,dimmed:!1};e.push({...m,color:[M(d,f*.8),M(d,f*.8)],alpha:f*.8,stroke:.55,dashLength:1,dashGap:0,dashInitialGap:0,animateTime:0}),e.push({...m,color:[M(d,0),M(`#d9ffe0`,u?.95:.6)],alpha:1,stroke:.62,dashLength:.22,dashGap:.78,dashInitialGap:(t*3+n)*.37%1,animateTime:B(2600+p*45,2800,6500)})}return e}_buildArcsList(){if(this.simpleMode)return this._buildSimpleArcs();let e=!!(this.selectedServerId||this.selectedPairKey),t=[],n=[];for(let e of this.servers)for(let t of this.servers)e.id!==t.id&&n.push(`${e.id}->${t.id}`);n.forEach((n,r)=>{let[i,a]=n.split(`->`),o=this.serversById[i],s=this.serversById[a];if(!o||!s)return;let c=this.selectedPairKey===n&&this.selectedPairMeasurement?this.selectedPairMeasurement:this.pairData[n],l=this.selectedPairKey===n,u=this.selectedServerId&&(i===this.selectedServerId||a===this.selectedServerId),d=this.hoveredPairKey===n,f=e&&!l&&!u&&!d,p=c?c.status:`unknown`,m=A(p),h=c&&typeof c.dest_avg_rtt==`number`?c.dest_avg_rtt:70,g=c&&typeof c.dest_loss==`number`?c.dest_loss:0,_=B(950+h*20,1100,4600),v=p===`critical`,y=p===`serious`,b=v?.16:y?.24:.44,x=v?.42:y?.3:.18,S=v?Math.max(850,_*.72):_,C=i<a?1:-1,w=B(B(.045+V(o.lat,o.lon,s.lat,s.lon)/390,.06,.22)+(C>0?.036:-.018),.04,.26);if((this.routeMode||l)&&c&&Array.isArray(c.hops)&&c.hops.length>=2&&p!==`error`){let e=[{lat:o.lat,lon:o.lon,asn:o.asn,city:o.city}];for(let t of c.hops){if(typeof t.lat!=`number`||typeof t.lon!=`number`)continue;let n=e[e.length-1];V(n.lat,n.lon,t.lat,t.lon)<.22?t.primary_asn&&(n.asn=t.primary_asn):e.push({lat:t.lat,lon:t.lon,asn:t.primary_asn||n.asn,city:t.city||`Interpolated`,hop:t.hop,loss_class:t.loss_class})}let u=e[e.length-1];V(u.lat,u.lon,s.lat,s.lon)>=.22&&e.push({lat:s.lat,lon:s.lon,asn:s.asn,city:s.city});let m=r%6*.008+(C>0?.015:0);for(let r=0;r<e.length-1;r++){let o=e[r],s=e[r+1],u=s.asn||o.asn||c.primary_transit_asn,_=j(u,this.carriers),b=V(o.lat,o.lon,s.lat,s.lon),x=B(.02+b/320+m,.025,.28),C=_.color,w=f?.08:l||d?.96:.82,T=f?.22:l||d?.88:v?.85:.56;t.push({pairKey:n,srcId:i,dstId:a,startLat:o.lat,startLng:o.lon,endLat:s.lat,endLng:s.lon,altitude:x,color:[M(C,w*.65),M(C,w)],baseHex:C,alpha:w,stroke:T,dashLength:v?.18:.52,dashGap:v?.38:.14,dashInitialGap:r*.23%1,animateTime:B(S*Math.max(.35,b/45),750,3200),status:p,loss:g,rtt:h,carrierName:_.name,carrierAsn:u,fromCity:o.city,toCity:s.city,isRouteSegment:!0,isLossy:v||y,dimmed:f})}}else{let e=f?.07:l||d?.98:c?.84:.28,u=f?.2:l||d?.95:v?.9:y?.72:.52,_=m.color;t.push({pairKey:n,srcId:i,dstId:a,startLat:o.lat,startLng:o.lon,endLat:s.lat,endLng:s.lon,altitude:w,color:[M(_,e*.45),M(_,e)],baseHex:_,alpha:e,stroke:u,dashLength:b,dashGap:x,dashInitialGap:r*.17%1,animateTime:S,status:p,loss:g,rtt:h,carrierName:c?c.primary_transit_name:null,carrierAsn:c?c.primary_transit_asn:null,isRouteSegment:!1,isLossy:v||y,dimmed:f})}});let r=t.filter(e=>!e.dimmed).map(e=>({...e,isHit:!0,isLossy:!1,stroke:se,color:[`rgba(0, 0, 0, 0)`,`rgba(0, 0, 0, 0)`],alpha:0,dashLength:1,dashGap:0,dashInitialGap:0,animateTime:0}));return t.concat(r)}_keepSame(e,t,n){let r=new Map,i=t.map(t=>{let i=n(t);for(let e=1;r.has(i);e++)i=`${n(t)}#${e}`;let a=e.get(i);if(a){for(let e of Object.keys(a))!(e in t)&&!e.startsWith(`__`)&&delete a[e];Object.assign(a,t)}let o=a||t;return r.set(i,o),o});e.clear();for(let[t,n]of r)e.set(t,n);return i}_renderArcs(){let e=e=>`${Number(e.startLat).toFixed(3)},${Number(e.startLng).toFixed(3)}>${Number(e.endLat).toFixed(3)},${Number(e.endLng).toFixed(3)}`,t=this._keepSame(this._arcsKept,this._buildArcsList(),t=>`${t.pairKey||``}|${e(t)}|${t.isSimple?t.dashGap?`pulse`:`line`:``}|${t.isHit?`hit`:``}`);this.world.arcsData(t),requestAnimationFrame(()=>this._collectLossyArcMeshes()),clearTimeout(this._arcPassTimer),this._arcPassTimer=setTimeout(()=>this._collectLossyArcMeshes(),400)}_refreshArcsVisualState(){this._renderArcs()}_collectLossyArcMeshes(){this._arcMeshesLossy=[],this.world.scene().traverse(e=>{if(e.__globeObjType===`arc`&&e.__data&&e.__data.isHit){let t=e.children&&e.children[0];t&&t.material&&(t.material.colorWrite=!1,t.material.depthWrite=!1)}else if(e.__globeObjType===`arc`&&e.__data&&e.__data.isLossy&&!e.__data.dimmed){let t=e.children&&e.children[0]?e.children[0]:e;t&&t.material&&this._arcMeshesLossy.push({mat:t.material,status:e.__data.status,phase:(e.__data.pairKey.charCodeAt(1)+e.__data.pairKey.charCodeAt(4))*1.7})}})}_renderHopMarkers(){this._hopMeshes.clear();let e=[];if(this.simpleMode){this.world.objectsData(e);return}let t=[];if(this.selectedPairKey){let e=this.selectedPairMeasurement||this.pairData[this.selectedPairKey];e&&Array.isArray(e.hops)&&e.hops.length&&t.push([this.selectedPairKey,e])}else if(this.routeMode)for(let[e,n]of Object.entries(this.pairData||{}))n&&Array.isArray(n.hops)&&n.hops.length&&n.status!==`error`&&t.push([e,n]);let n=new Map;for(let[r,i]of t){let[t,a]=r.split(`->`),o=this.serversById[t],s=this.serversById[a];if(o&&s)for(let t of i.hops){if(typeof t.lat!=`number`||typeof t.lon!=`number`)continue;let i=V(t.lat,t.lon,o.lat,o.lon)<.15,a=V(t.lat,t.lon,s.lat,s.lon)<.15;if((i||a)&&t.loss_class!==`culprit`&&!this.selectedPairKey)continue;let c=`${t.lat.toFixed(1)},${t.lon.toFixed(1)}`,l=n.get(c)||0;n.set(c,l+1);let u=t.lat,d=t.lon;if(l>0&&this.selectedPairKey){let e=l*1.45,t=Math.min(.45,.14+l*.06);u+=Math.sin(e)*t,d+=Math.cos(e)*t*1.3}let f=j(t.primary_asn,this.carriers);e.push({markerKey:`${r}#${t.hop}`,pairKey:r,hop:t.hop,lat:u,lon:d,alt:t.loss_class===`culprit`?.028:t.located?.016:.014,located:!!t.located,isSilent:!!t.is_silent,lossClass:t.loss_class,loss:t.stats?t.stats.loss:0,avgRtt:t.stats?t.stats.avg:null,hostname:t.resolvedHostname,ip:t.resolvedAddress,city:t.city,carrierName:t.carrier_name||f.name,carrierAsn:t.primary_asn,color:t.loss_class===`culprit`?`#f87171`:t.is_silent?`#7e889b`:f.color,isSelectedPair:this.selectedPairKey===r})}}this.world.objectsData(e)}_createHopMesh(t){let n=new c,s=t.lossClass===`culprit`,l=t.isSelectedPair?.72:.48,u=new m(t.color);if(t.located){let e=new a(s?l*1.25:l,14,14),o=new i({color:u,transparent:!0,opacity:t.isSelectedPair?.96:.78});n.add(new r(e,o))}else{let o=new e(l*.95,l*.24,10,24),s=new i({color:u,transparent:!0,opacity:t.isSelectedPair?.9:.65});n.add(new r(o,s));let c=new a(l*1.15,8,8),d=new i({transparent:!0,opacity:.001,depthWrite:!1});n.add(new r(c,d))}let d=new o(l*1.35,l*1.85,24),f=new i({color:s?`#ff4f4f`:`#f0efec`,side:2,transparent:!0,opacity:s?.92:.85}),p=new r(d,f);return p.visible=s||this.hoveredHopIndex!==null&&t.hop===this.hoveredHopIndex,n.add(p),this._hopMeshes.set(t.markerKey,{group:n,halo:p,hop:t.hop,pairKey:t.pairKey,isCulprit:s,isHovered:p.visible&&!s}),s&&n.scale.setScalar(1.35),n}_buildServerTooltip(e){let t=A(e.healthStatus);if(this.simpleMode){let n=this.validatorInfo[e.id]||{},r=n.up===!1?`Not answering right now`:`Up and running`;return`
        <div class="globe-tooltip">
          <div class="gt-title">
            <span class="gt-dot" style="background:${t.color}"></span>
            <strong>Validator ${N(n.number||e.name)}</strong> · ${N(e.city)}, ${N(e.country)}
          </div>
          <div class="gt-row">${r}</div>
        </div>
      `}return`
      <div class="globe-tooltip">
        <div class="gt-title">
          <span class="gt-dot" style="background:${t.color}"></span>
          <strong>${N(e.name)}</strong> · ${N(e.city)}, ${N(e.country)}
        </div>
        <div class="gt-row">${N(e.host)} (${N(e.ip)})</div>
        <div class="gt-row">${N(e.network)}</div>
        <div class="gt-hint">Click to zoom &amp; inspect BGP</div>
      </div>
    `}_buildArcTooltip(e){let t=A(e.status);if(e.isSimple){let t=e.status===`unknown`?`Link not measured yet`:e.status===`good`?`Healthy link`:`Link losing ${Number(e.loss||0).toFixed(0)}% of test messages`,n=typeof e.rtt==`number`?` · ${Math.round(e.rtt)} ms`:``;return`
        <div class="globe-tooltip">
          <div class="gt-title">
            <span class="gt-dot" style="background:${e.baseHex}"></span>
            <strong>${N(e.fromCity)} ↔ ${N(e.toCity)}</strong>
          </div>
          <div class="gt-row">${t}${n}</div>
        </div>
      `}let n=e.status===`unknown`?`No data`:`${Number(e.loss||0).toFixed(1)}% loss`,r=typeof e.rtt==`number`?`${e.rtt.toFixed(1)} ms`:`—`,i=e.carrierName?`<div class="gt-row">Carrier: <strong>${N(e.carrierName)}</strong>${e.carrierAsn?` (AS${e.carrierAsn})`:``}</div>`:``,a=e.isRouteSegment?`<div class="gt-row">${N(e.fromCity||``)} → ${N(e.toCity||``)}</div>`:``;return`
      <div class="globe-tooltip">
        <div class="gt-title">
          <span class="gt-dot" style="background:${t.color}"></span>
          <strong>${N(e.srcId)} → ${N(e.dstId)}</strong>
          <span class="gt-badge" style="color:${t.color}">${t.icon} ${t.label}</span>
        </div>
        <div class="gt-row">${n} · ${r}</div>
        ${i}
        ${a}
        <div class="gt-hint">Click to inspect hop-by-hop subway route</div>
      </div>
    `}_buildHopTooltip(e){let t=e.hostname||e.ip||`* (no reply)`,n=e.located?`${N(e.city||`Located`)}`:`Interpolated position`,r=typeof e.avgRtt==`number`?`${e.avgRtt.toFixed(1)} ms`:`—`,i=e.lossClass===`rate_limited`?`<span style="color:#9ca3af">${e.loss}% (ICMP rate limit)</span>`:e.lossClass===`culprit`?`<span style="color:#ff6b6b"><strong>${e.loss}% · LIKELY CULPRIT</strong></span>`:e.lossClass===`silent`?`<span style="color:#9ca3af">Silent (*)</span>`:`${e.loss}% loss`;return`
      <div class="globe-tooltip">
        <div class="gt-title">
          <span class="gt-dot" style="background:${e.color}"></span>
          <strong>Hop ${e.hop}</strong> (${N(e.pairKey)})
        </div>
        <div class="gt-mono">${N(t)}</div>
        <div class="gt-row">${N(e.carrierName||`Unknown`)}${e.carrierAsn?` · AS${e.carrierAsn}`:``}</div>
        <div class="gt-row">${r} · ${i}</div>
        <div class="gt-hint">${n}</div>
      </div>
    `}_startEffectsLoop(){if(this._fxRaf)return;let e=t=>{if(this.isHidden)this._fxRaf=null;else{if(!this.reducedMotion){if(this._starfield&&(this._starfield.rotation.y=t*12e-6),this.simpleMode){if(this._lastFxTime){let e=B(1e3/Math.max(1,t-this._lastFxTime),20,240);this._fps=this._fps?this._fps*.9+e*.1:e;let n=this.world.controls();n.autoRotateSpeed=(n.autoRotateSpeed<0?-1:1)*R/(.1*this._fps)}this._swayTick()}if(this._lastFxTime=t,this._arcMeshesLossy.length>0){let e=t*.001;for(let t of this._arcMeshesLossy){if(!t.mat)continue;let n=Math.sin(e*(t.status===`critical`?14.5:8.5)+t.phase)*.5+Math.sin(e*23+t.phase*1.9)*.5,r=n>.35?1:n<-.3?.32:.72;t.mat.transparent=!0,t.mat.opacity=r}}for(let e of this._hopMeshes.values())if(e.isCulprit&&!e.isHovered&&e.halo){let n=1+.35*Math.sin(t*.007);e.halo.scale.setScalar(n)}}this._fxRaf=requestAnimationFrame(e)}};this._fxRaf=requestAnimationFrame(e)}destroy(){this._cancelFlight(),this._fxRaf&&cancelAnimationFrame(this._fxRaf),this._idleTimer&&clearTimeout(this._idleTimer),window.removeEventListener(`resize`,this._onResize),document.removeEventListener(`visibilitychange`,this._onVisibility),this.world&&this.world._destructor&&this.world._destructor()}},W=`249, 221, 164`,G=`184, 199, 255`,fe=`11, 11, 15`,K=Math.PI*2,pe=150,me=100,he=`"Inter", "Segoe UI Variable", "Segoe UI", system-ui, -apple-system, sans-serif`;function q(e){if(e==null)return`—`;if(e>0&&e<.001)return`under 0.001`;let t=e>=100?0:e>=1?1:3;return e.toLocaleString(`en-US`,{maximumFractionDigits:t,minimumFractionDigits:0})}function J(e){return e?`${e.slice(0,6)}…${e.slice(-4)}`:``}function Y(e){let t=Array(e),n=Math.floor((e-1)/2);for(let r=0;r<e;r++){let e=Math.ceil(r/2);t[r]=r%2==1?n+e:n-e}let r=new Set(t.filter(t=>t>=0&&t<e));for(let n=0;n<e;n++)if(t[n]<0||t[n]>=e){for(let i=0;i<e;i++)if(!r.has(i)){t[n]=i,r.add(i);break}}return t}var ge=class{constructor(e,t,{getGlobeCircle:n,onSelect:r,hostEl:i}){this.canvas=e,this.ctx=e.getContext(`2d`),this.tipEl=t,this.getGlobeCircle=n,this.onSelect=r,this.hostEl=i,this.visible=!1,this.nodes=[],this.minerCount=0,this.walletCount=0,this.layout=null,this.hovered=null,this.selectedKey=null,this.reducedMotion=window.matchMedia(`(prefers-reduced-motion: reduce)`).matches,this._raf=null,this._onMove=e=>this._handleMove(e),this._onClick=e=>this._handleClick(e),this._onLeave=()=>this._setHovered(null),window.addEventListener(`mousemove`,this._onMove),window.addEventListener(`click`,this._onClick),document.addEventListener(`mouseleave`,this._onLeave),document.addEventListener(`visibilitychange`,()=>{!document.hidden&&this.visible&&this._start()})}setVisible(e){this.visible=!!e,this.canvas.hidden=!this.visible,this.visible?this._start():this._setHovered(null)}setSelected(e){this.selectedKey=e||null}setLayout({cx:e,cy:t,minerR:n,walletR:r,scale:i}){this.layout={cx:e,cy:t,minerR:n,walletR:r,scale:i};let a=Math.min(window.devicePixelRatio||1,2),o=this.hostEl.clientWidth||window.innerWidth,s=this.hostEl.clientHeight||window.innerHeight;this._w=o,this._h=s,this.canvas.width=Math.round(o*a),this.canvas.height=Math.round(s*a),this.canvas.style.width=`${o}px`,this.canvas.style.height=`${s}px`,this._dpr=a,this._place()}setData(e){let t=[],n=e.miners||[],r=n.slice(0,pe),i=(e.wallets||[]).slice(0,me),a=Math.max(...r.map(e=>e.part),1e-4);r.forEach((e,n)=>{t.push({key:`m:${e.address}`,kind:`miner`,rank:n,address:e.address,mined:e.mined,part:e.part,holds:e.holds,weight:Math.sqrt(e.part/a)})});let o=Math.max(...i.map(e=>e.holds),1e-4);i.forEach((e,n)=>{t.push({key:`w:${e.address}`,kind:`wallet`,rank:n,address:e.address,holds:e.holds,share:e.share,mining:!!e.mining,weight:Math.sqrt(e.holds/o)})}),this.nodes=t,this.minerCount=r.length,this.walletCount=i.length,this.minerTotal=n.length,this.walletTotal=Math.max(e.holding||0,i.length),this._place()}_place(){if(!this.layout)return;let{cx:e,cy:t,minerR:n,walletR:r,scale:i}=this.layout;this._labelSize=Math.max(11,Math.round(12.5*i));let a=(e,t)=>{let n=this._measure(e)+52*i;return 2*Math.asin(Math.min(.9,n/2/t))},o={miner:a(this._minerLabel(),n),wallet:a(this._walletLabel(),r)},s=Y(this.minerCount),c=Y(this.walletCount);for(let a of this.nodes){let l=a.kind===`miner`,u=l?this.minerCount:this.walletCount,d=l?s[a.rank]:c[a.rank],f=l?n:r,p=l?o.miner:o.wallet,m=u>1?d/(u-1):.5;a.angle=Math.PI/2+p/2+m*(K-p),a.x=e+Math.cos(a.angle)*f,a.y=t+Math.sin(a.angle)*f,a.ringR=f,a.r=(3+6.2*a.weight)*i,a.phase=a.rank*.61803%1}}_ringLabel(e,t,n,r){let i=t.toLocaleString(`en-US`);return e<t?`${e} LARGEST OF ${i} ${r}`:`${i} ${t===1?n:r}`}_minerLabel(){return this._ringLabel(this.minerCount,this.minerTotal||this.minerCount,`MINER`,`MINERS`)}_walletLabel(){return this._ringLabel(this.walletCount,this.walletTotal||this.walletCount,`WALLET`,`WALLETS`)}_labelFont(){let e=Date.now();return(!this._family||e-(this._familyAt||0)>2e3)&&(this._family=getComputedStyle(this.canvas).fontFamily||he,this._familyAt=e),`600 ${this._labelSize||12}px ${this._family}`}_measure(e){let t=this.ctx;t.save(),t.setTransform(1,0,0,1,0,0),t.font=this._labelFont(),`letterSpacing`in t&&(t.letterSpacing=`0.09em`);let n=t.measureText(e).width;return t.restore(),`letterSpacing`in t&&(t.letterSpacing=`0px`),n}_nodeAt(e,t){let n=null,r=1/0;for(let i of this.nodes){let a=Math.hypot(i.x-e,i.y-t);a<=Math.max(i.r+6,10)&&a<r&&(n=i,r=a)}return n}_overMap(e){return this.visible&&this.hostEl&&this.hostEl.contains(e.target)}_handleMove(e){if(!this._overMap(e)){this._setHovered(null);return}let t=this._within(e);this._setHovered(this._nodeAt(t.x,t.y),t.x,t.y)}_within(e){let t=this.hostEl.getBoundingClientRect();return{x:e.clientX-t.left,y:e.clientY-t.top}}_handleClick(e){if(!this._overMap(e))return;let t=this._within(e),n=this._nodeAt(t.x,t.y);n&&this.onSelect&&this.onSelect(n)}_setHovered(e,t=0,n=0){let r=(e&&e.key)!==(this.hovered&&this.hovered.key);if(this.hovered=e,this.hostEl&&this.hostEl.classList.toggle(`has-orbit-hover`,!!e),!e){this.tipEl.hidden=!0;return}r&&(this.tipEl.innerHTML=this._tipHtml(e)),this.tipEl.hidden=!1;let i=this.tipEl.offsetWidth,a=this.tipEl.offsetHeight,o=Math.min(Math.max(12,t+16),(this._w||window.innerWidth)-i-12),s=Math.min(Math.max(12,n+18),(this._h||window.innerHeight)-a-12);this.tipEl.style.transform=`translate(${Math.round(o)}px, ${Math.round(s)}px)`}_tipHtml(e){return e.kind===`miner`?`
        <div class="ot-title"><span class="ot-dot ot-miner"></span><strong>Miner</strong><span class="mono">${N(J(e.address))}</span></div>
        <div class="ot-row">Mined ${q(e.mined)} ORN in the last hour · ${(e.part*100).toFixed(1)}% of it</div>
        ${e.holds!==null&&e.holds!==void 0?`<div class="ot-row">Holds ${q(e.holds)} ORN</div>`:``}
      `:`
      <div class="ot-title"><span class="ot-dot ot-wallet"></span><strong>Wallet</strong><span class="mono">${N(J(e.address))}</span></div>
      <div class="ot-row">Holds ${q(e.holds)} ORN${e.mining?` · mining this hour`:``}</div>
    `}_start(){if(this._raf)return;let e=t=>{this._raf=null,this.visible&&!document.hidden&&(this._draw(t),this._raf=requestAnimationFrame(e))};this._raf=requestAnimationFrame(e)}_draw(e){let t=this.ctx,n=this._dpr||1;if(t.setTransform(n,0,0,n,0,0),t.clearRect(0,0,this._w||window.innerWidth,this._h||window.innerHeight),!this.layout||!this.nodes.length)return;let{cx:r,cy:i,minerR:a,walletR:o,scale:s}=this.layout,c=this.getGlobeCircle().r*1.05,l=this.hovered&&this.hovered.key||this.selectedKey,u=!!l;t.lineCap=`round`;for(let n of this.nodes){let a=n.key===l,o=n.kind===`miner`?W:G,d=n.kind===`miner`?.5:.3,f=n.kind===`miner`?.16:.08;a?(d=1,f=.7):u&&(d*=.55,f*=.55);let p=Math.cos(n.angle),m=Math.sin(n.angle),h=r+p*(n.ringR-n.r-3*s),g=i+m*(n.ringR-n.r-3*s),_=r+p*c,v=i+m*c,y=t.createLinearGradient(h,g,_,v);if(y.addColorStop(0,`rgba(${o}, ${d})`),y.addColorStop(1,`rgba(${o}, ${f})`),t.strokeStyle=y,t.lineWidth=a?1.6:1,t.beginPath(),t.moveTo(h,g),t.lineTo(_,v),t.stroke(),n.kind===`miner`&&!this.reducedMotion){let o=((e/(4200+n.phase*2600)+n.phase)%1+1)%1,l=n.ringR-n.r-3*s-o*(n.ringR-n.r-3*s-c),d=Math.sin(o*Math.PI);t.fillStyle=`rgba(255, 226, 150, ${(u&&!a?.3:.85)*d})`,t.beginPath(),t.arc(r+p*l,i+m*l,1.3*s,0,K),t.fill()}}for(let e of this.nodes){let n=e.key===l,r=u&&!n,i=e.kind===`miner`?W:G;t.fillStyle=`rgba(${fe}, 1)`,t.beginPath(),t.arc(e.x,e.y,e.r+2.5*s,0,K),t.fill(),r||(t.shadowColor=`rgba(${i}, ${n?.9:.55})`,t.shadowBlur=(n?16:8)*s),t.fillStyle=`rgba(${i}, ${r?.4:1})`,t.beginPath(),t.arc(e.x,e.y,e.r,0,K),t.fill(),t.shadowBlur=0,n&&(t.strokeStyle=`rgba(255, 255, 255, 0.9)`,t.lineWidth=1.5,t.beginPath(),t.arc(e.x,e.y,e.r+4*s,0,K),t.stroke())}t.font=this._labelFont(),t.textAlign=`center`,t.textBaseline=`middle`,`letterSpacing`in t&&(t.letterSpacing=`0.09em`),t.fillStyle=`rgba(${W}, 0.95)`,t.fillText(this._minerLabel(),r,i+a),t.fillStyle=`rgba(${G}, 0.95)`,t.fillText(this._walletLabel(),r,i+o),`letterSpacing`in t&&(t.letterSpacing=`0px`)}},_e=`https://explorer.orien.network/address/`;function X(e,t,n){return e===1?t:n}function ve(e){if(!e)return``;let t=Math.max(0,Math.round((Date.now()/1e3-e)/60));if(t<1)return`just now`;if(t<60)return`${t} min ago`;let n=Math.floor(t/60);return t%60?`${n} h ${t%60} min ago`:`${n} h ago`}function ye(e){if(!e||!e.ready)return`
      <div class="sc-head">
        <span class="sc-title">Orien network</span>
      </div>
      <p class="sc-status">Reading the network…</p>
    `;let t=e.validators||[],n=t.filter(e=>e.up).length,r=t.length,i=e.checkpoint||{},a=i.of&&i.agreed===i.of,o;o=n===r&&a?`All ${r} validators are up and agreeing.`:n===r?`All ${r} validators are up.`:`${n} of ${r} validators are up.`;let s=e.fetched_at?Math.floor((Date.now()/1e3-e.fetched_at)/60):0,c=s<5,l=(e.miners||[]).length,u=e.holding||(e.wallets||[]).length,d=(e.wallets||[]).length,f=u>d?` The outer ring shows the ${d} largest wallets.`:``,p=i.epoch!==void 0&&i.epoch!==null?`Checkpoint ${i.epoch} closed ${ve(i.time)}`:``;return`
    <div class="sc-head">
      <span class="sc-title">Orien network</span>
      <span class="sc-live"><span class="sc-live-dot${n===r&&c?``:` is-warn`}"></span>${c?`Live`:`Read ${s} min ago`}</span>
    </div>
    <p class="sc-status">${N(o)}</p>

    <ul class="sc-key">
      <li>
        <span class="sc-swatch is-validator"></span>
        <span class="sc-num">${r}</span>
        <span class="sc-what"><strong>${X(r,`Validator`,`Validators`)}</strong><span>on the globe, where they are</span></span>
      </li>
      <li>
        <span class="sc-swatch is-miner"></span>
        <span class="sc-num">${l}</span>
        <span class="sc-what"><strong>${X(l,`Miner`,`Miners`)}</strong><span>wallets that mined in the last hour</span></span>
      </li>
      <li>
        <span class="sc-swatch is-wallet"></span>
        <span class="sc-num">${u}</span>
        <span class="sc-what"><strong>${X(u,`Wallet`,`Wallets`)}</strong><span>hold ORN · ${l} of them mining</span></span>
      </li>
    </ul>

    <p class="sc-note">Each line is one miner or wallet on the network. They circle the globe because the network does not record where anyone is.${f}</p>
    ${p?`<p class="sc-foot">${N(p)}</p>`:``}
  `}function Z(e,t,n,r,i=``){return`
    <div class="sd-head">
      <span class="sc-swatch ${e}"></span>
      <div class="sd-titles">
        <span class="sd-title">${t}</span>
        ${n?`<span class="sd-sub">${n}</span>`:``}
      </div>
      <button type="button" class="hud-btn hud-btn-ghost sd-close" data-close-detail aria-label="Close">Close</button>
    </div>
    <div class="sd-rows">${r.map(e=>`<p class="sd-row">${e}</p>`).join(``)}</div>
    ${i}
  `}function Q(e){return`<a class="hud-btn hud-btn-primary sd-action" href="${_e}${encodeURIComponent(e)}/" target="_blank" rel="noopener">Open in the explorer</a>`}function be(e){if(!e)return``;if(e.type===`validator`){let{server:t,info:n,links:r}=e,i=[];return n&&(i.push(n.up?`Up and running.`:`Not answering right now.`),n.of&&i.push(`Agreed on ${n.agreed} of the last ${n.of} checkpoints.`)),r&&r.length&&i.push(`Reaches ${r.map(e=>`${N(e.city)} in ${Math.round(e.rtt)} ms`).join(`, `)}.`),Z(n&&n.up===!1?`is-down`:`is-validator`,`Validator ${N(n?n.number:t.name)}`,`${N(t.city)}, ${N(t.country)}`,i)}let t=e.node;if(t.kind===`miner`){let e=[`Mined <strong>${q(t.mined)} ORN</strong> in the last hour, ${(t.part*100).toFixed(1)}% of all that was mined.`];return t.holds!==null&&t.holds!==void 0&&e.push(`Holds ${q(t.holds)} ORN.`),Z(`is-miner`,`Miner`,`<span class="mono">${N(J(t.address))}</span>`,e,Q(t.address))}let n=[`Holds <strong>${q(t.holds)} ORN</strong>, ${(t.share*100).toFixed(t.share>=.01?1:2)}% of all ORN in wallets.`];return t.mining&&n.push(`Mining this hour.`),Z(`is-wallet`,`Wallet`,`<span class="mono">${N(J(t.address))}</span>`,n,Q(t.address))}var $=[{value:0,label:`Auto: Off`},{value:5,label:`Every 5m`},{value:10,label:`Every 10m`},{value:15,label:`Every 15m`},{value:30,label:`Every 30m`},{value:60,label:`Every 60m`}],xe=class{constructor(e,t={}){this.container=e,this.callbacks=t,this._state=null,this._routeMode=!1,this._hasFocus=!1,this._countdownTimer=null,this._renderSkeleton(),this._bindEvents(),this._startCountdownTick()}_renderSkeleton(){let e=$.map(e=>`<option value="${e.value}">${e.label}</option>`).join(``);this.container.innerHTML=`
      <div class="hud-left">
        <div class="brand-block">
          <span class="brand-dot" id="hdr-brand-dot"></span>
          <div class="brand-text">
            <span class="brand-title">NETWORK DETAILS</span>
            <span class="brand-sub" id="hdr-brand-sub">4 NODES · 12 PATHS</span>
          </div>
        </div>

        <div class="mode-toggle" role="group" aria-label="Arc display mode">
          <button type="button" class="mode-btn is-active" id="btn-mode-direct" data-mode="direct" title="Great-circle direct arcs colored by path health">
            Direct
          </button>
          <button type="button" class="mode-btn" id="btn-mode-route" data-mode="route" title="Multi-hop real routes colored by carrier/ASN">
            Real Route
          </button>
        </div>
      </div>

      <div class="hud-center">
        <div class="cycle-meta">
          <div class="meta-item">
            <span class="meta-label">LAST CYCLE</span>
            <span class="meta-val mono" id="hdr-last-cycle">Never</span>
          </div>
          <span class="meta-sep" aria-hidden="true"></span>
          <div class="meta-item">
            <span class="meta-label">NEXT</span>
            <span class="meta-val mono" id="hdr-countdown">—</span>
          </div>
          <span class="meta-sep" aria-hidden="true"></span>
          <div class="meta-item" id="hdr-quota-wrap" title="Globalping hourly test allowance">
            <span class="meta-label">QUOTA</span>
            <span class="meta-val mono" id="hdr-quota">—</span>
          </div>
        </div>

        <div class="progress-ticks" id="hdr-progress-ticks" hidden aria-label="Per-pair measurement progress"></div>
      </div>

      <div class="hud-right">
        <label class="sr-only" for="hdr-interval-select">Auto-refresh interval</label>
        <select id="hdr-interval-select" class="hud-select" title="Auto-refresh interval (Globalping free tier: 250 tests/hr)">
          ${e}
        </select>

        <button type="button" id="btn-refresh" class="hud-btn hud-btn-primary">
          <span class="btn-spinner" aria-hidden="true"></span>
          <span class="btn-label">Refresh now</span>
        </button>

        <button type="button" id="btn-reset-view" class="hud-btn hud-btn-ghost" title="Reset camera to overview (Esc)">
          Reset view <kbd class="kbd">Esc</kbd>
        </button>

        <button type="button" id="btn-simple-view" class="hud-btn hud-btn-ghost" title="Back to the simple map">
          Simple view
        </button>
      </div>
    `,this.els={brandDot:this.container.querySelector(`#hdr-brand-dot`),brandSub:this.container.querySelector(`#hdr-brand-sub`),btnDirect:this.container.querySelector(`#btn-mode-direct`),btnRoute:this.container.querySelector(`#btn-mode-route`),lastCycle:this.container.querySelector(`#hdr-last-cycle`),countdown:this.container.querySelector(`#hdr-countdown`),quotaWrap:this.container.querySelector(`#hdr-quota-wrap`),quota:this.container.querySelector(`#hdr-quota`),progressTicks:this.container.querySelector(`#hdr-progress-ticks`),intervalSelect:this.container.querySelector(`#hdr-interval-select`),btnRefresh:this.container.querySelector(`#btn-refresh`),btnReset:this.container.querySelector(`#btn-reset-view`),btnSimple:this.container.querySelector(`#btn-simple-view`)}}_bindEvents(){this.els.btnDirect.addEventListener(`click`,()=>{this._routeMode&&this.callbacks.onRouteModeChange&&this.callbacks.onRouteModeChange(!1)}),this.els.btnRoute.addEventListener(`click`,()=>{!this._routeMode&&this.callbacks.onRouteModeChange&&this.callbacks.onRouteModeChange(!0)}),this.els.intervalSelect.addEventListener(`change`,e=>{let t=parseInt(e.target.value,10);!Number.isNaN(t)&&this.callbacks.onIntervalChange&&this.callbacks.onIntervalChange(t)}),this.els.btnRefresh.addEventListener(`click`,()=>{this.callbacks.onRefresh&&this.callbacks.onRefresh()}),this.els.btnSimple.addEventListener(`click`,()=>{this.callbacks.onSimpleView&&this.callbacks.onSimpleView()}),this.els.btnReset.addEventListener(`click`,()=>{this.callbacks.onResetView&&this.callbacks.onResetView()})}_startCountdownTick(){this._countdownTimer=setInterval(()=>{this._updateCountdownText()},1e3)}_updateCountdownText(){let e=this._state&&this._state.scheduler;if(!e)return;if(e.running){let t=e.progress||{};this.els.countdown.textContent=`${t.completed||0}/${t.total||12} pairs`;return}if(!e.interval_minutes||e.interval_minutes<=0||!e.next_run_epoch){this.els.countdown.textContent=`Manual`;return}let t=Math.max(0,Math.round(e.next_run_epoch-Date.now()/1e3));this.els.countdown.textContent=F(t)}update({state:e,routeMode:t,hasFocus:n,activeIncidents:r}){this._state=e,this._routeMode=!!t,this._hasFocus=!!n,this.els.btnDirect.classList.toggle(`is-active`,!this._routeMode),this.els.btnRoute.classList.toggle(`is-active`,this._routeMode),this.els.btnReset.classList.toggle(`is-highlighted`,this._hasFocus);let i=e.servers||[],a=i.length*Math.max(0,i.length-1);this.els.brandSub.textContent=`${i.length} NODES · ${a} PATHS`;let o=(Array.isArray(r)?r:[]).filter(e=>e.lasting),s=o.some(e=>e.reached===!1)?`#fb923c`:o.length?`#fbbf24`:`#4ade80`;this.els.brandDot.style.background=s,this.els.brandDot.style.boxShadow=`0 0 10px ${s}`;let c=e.scheduler||{};this.els.lastCycle.textContent=c.last_cycle_at?P(c.last_cycle_at):`No runs yet`;let l=String(c.interval_minutes??10);if(this.els.intervalSelect.value!==l){if(![...this.els.intervalSelect.options].some(e=>e.value===l)){let e=document.createElement(`option`);e.value=l,e.textContent=`Every ${l}m`,this.els.intervalSelect.appendChild(e)}this.els.intervalSelect.value=l}let u=!!c.running;this.els.btnRefresh.disabled=u,this.els.btnRefresh.classList.toggle(`is-loading`,u);let d=this.els.btnRefresh.querySelector(`.btn-label`),f=c.progress||{};if(d.textContent=u?`Measuring ${f.completed||0}/${f.total||a}`:`Refresh now`,u&&f.pairs){this.els.progressTicks.hidden=!1;let e=Object.entries(f.pairs);this.els.progressTicks.innerHTML=e.map(([e,t])=>{let n=`tick-pending`,r=`rgba(195,194,183,0.22)`,i=`${e}: pending`;if(t.status===`running`)n=`tick-running`,r=`#d3c4ff`,i=`${e}: measuring...`;else if(t.status===`done`){let a=A(t.health||`good`);n=`tick-done`,r=a.color,i=`${e}: ${t.dest_loss??0}% loss · ${t.dest_avg_rtt??`—`} ms`}else t.status===`error`&&(n=`tick-error`,r=`#f87171`,i=`${e}: error — ${t.error||`failed`}`);return`<span class="prog-tick ${n}" style="background:${r}" title="${N(i)}"></span>`}).join(``)}else this.els.progressTicks.hidden=!0,this.els.progressTicks.innerHTML=``;let p=e.quota||{};if(p.remaining!==null&&p.remaining!==void 0&&p.limit){this.els.quota.textContent=`${p.remaining}/${p.limit}`;let e=p.reset_seconds?`${Math.ceil(p.reset_seconds/60)}m`:`—`;this.els.quotaWrap.title=`Globalping API quota (${p.authenticated?`Authenticated token`:`Unauthenticated IP`}): ${p.remaining} of ${p.limit} tests remaining (resets in ~${e})`,this.els.quota.classList.toggle(`is-low`,p.remaining<36)}else this.els.quota.textContent=`—`;this._updateCountdownText()}},Se=class{constructor(e,t={}){this.container=e,this.callbacks=t,this._currentPair=null,this.container.addEventListener(`click`,e=>{let t=e.target.closest(`[data-inspect-pair]`);t&&this.callbacks.onInspectPair&&this.callbacks.onInspectPair(t.dataset.inspectPair)})}update(e=[],t=!1){if(!e||e.length===0){this.container.classList.remove(`is-visible`,`is-lasting`,`is-brief`),this.container.hidden=!0,this._currentPair=null,w().classList.remove(`has-incident-banner`);return}let n=e[0],r=!!n.lasting;this._currentPair=n.pair;let i=e=>[e.src,e.dst].sort().join(`|`),a=new Set(e.map(i).filter(e=>e!==i(n))).size,o=N(n.src),s=N(n.dst),c=Math.max(1,Number(n.rounds)||1)*10,l=t?`Earlier: `:``,u=n.culprit_carrier?` The loss starts on ${N(n.culprit_carrier)}'s network.`:``,d=a>0?` <span class="inc-extra">+${a} more</span>`:``,f,p;r?n.reached===!1?(f=`${l}${s} has not answered tests from ${o} for ${c} minutes`,p=`The other validators keep the network running while this path is checked.`):(f=`${l}The internet path ${o} → ${s} is losing packets`,p=`About ${Number(n.loss).toFixed(0)}% of test packets lost for ${c} minutes.${u} The network keeps running.`):(f=`${l}A brief blip between ${o} and ${s}`,p=`One test round lost a few packets. It usually passes by itself.`),this.container.innerHTML=`
      <div class="inc-inner">
        <span class="inc-icon" aria-hidden="true"></span>
        <div class="inc-body">
          <div class="inc-headline"><strong class="inc-pair">${f}</strong>${r?d:``}</div>
          <div class="inc-detail">${p}${r?``:d}</div>
        </div>
        <button type="button" class="inc-action" data-inspect-pair="${N(n.pair)}">See path</button>
      </div>
    `,this.container.classList.toggle(`is-lasting`,r),this.container.classList.toggle(`is-brief`,!r),this.container.setAttribute(`role`,r?`status`:`note`),this.container.setAttribute(`aria-live`,`polite`),this.container.hidden=!1,w().classList.add(`has-incident-banner`),requestAnimationFrame(()=>{this.container.classList.add(`is-visible`)})}};function Ce(e=[]){if(!e||e.length===0)return`<svg class="cell-spark" viewBox="0 0 54 16" aria-hidden="true"><line x1="0" y1="14" x2="54" y2="14" stroke="rgba(195,194,183,0.16)" stroke-width="1" stroke-dasharray="2 2"/></svg>`;let t=e.slice(-32),n=Math.max(20,...t.map(e=>typeof e.loss==`number`?e.loss:0)),r=14-10/n*12,i=t.map((e,r)=>{let i=t.length===1?27:r/(t.length-1)*52+1,a=typeof e.loss==`number`?e.loss:0;return{x:i,y:14-a/n*12,loss:a}}),a=i.map(e=>`${e.x.toFixed(1)},${e.y.toFixed(1)}`).join(` `),o=t.some(e=>(e.loss||0)>10)?`#f87171`:t.some(e=>(e.loss||0)>0)?`#fbbf24`:`#4ade80`,s=i.filter(e=>e.loss>10).map(e=>`<circle cx="${e.x.toFixed(1)}" cy="${e.y.toFixed(1)}" r="1.6" fill="#f87171"/>`).join(``);return`
    <svg class="cell-spark" viewBox="0 0 54 16" aria-hidden="true">
      <line x1="0" y1="${r.toFixed(1)}" x2="54" y2="${r.toFixed(1)}" stroke="rgba(208,59,59,0.22)" stroke-width="0.7" stroke-dasharray="2 2"/>
      ${t.length===1?`<circle cx="${i[0].x.toFixed(1)}" cy="${i[0].y.toFixed(1)}" r="2" fill="${o}"/>`:`<polyline fill="none" stroke="${o}" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" points="${a}"/>`}
      ${s}
    </svg>
  `}var we=class{constructor(e,t={}){this.container=e,this.callbacks=t,this.collapsed=!1,this._prevValues=new Map,this._reducedMotion=window.matchMedia(`(prefers-reduced-motion: reduce)`).matches,this._serverIdsKey=``,this._selectedPairKey=null,this.container.addEventListener(`click`,e=>{if(e.target.closest(`#btn-matrix-toggle`)){this.setCollapsed(!this.collapsed);return}let t=e.target.closest(`[data-server-id]`);if(t&&this.callbacks.onServerClick){this.callbacks.onServerClick(t.dataset.serverId);return}let n=e.target.closest(`[data-pair-key]`);n&&this.callbacks.onPairClick&&this.callbacks.onPairClick(n.dataset.pairKey)}),this.container.addEventListener(`mouseover`,e=>{let t=e.target.closest(`[data-pair-key]`);t&&this.callbacks.onPairHover&&this.callbacks.onPairHover(t.dataset.pairKey)}),this.container.addEventListener(`mouseleave`,()=>{this.callbacks.onPairHover&&this.callbacks.onPairHover(null)})}setCollapsed(e){this.collapsed=!!e,this.container.classList.toggle(`is-collapsed`,this.collapsed);let t=this.container.querySelector(`#btn-matrix-toggle`);t&&(t.setAttribute(`aria-expanded`,String(!this.collapsed)),t.textContent=this.collapsed?`Show`:`Hide`)}_animateNumber(e,t,n,r,i=0){if(!e)return;if(this._reducedMotion||t==null||Math.abs(n-t)<.05){e.textContent=`${n.toFixed(i)}${r}`;return}let a=performance.now(),o=s=>{let c=Math.min(1,(s-a)/420),l=1-(1-c)**3;e.textContent=`${(t+(n-t)*l).toFixed(i)}${r}`,c<1?requestAnimationFrame(o):e.textContent=`${n.toFixed(i)}${r}`};requestAnimationFrame(o)}update({servers:e,pairData:t,sparklines:n,selectedPairKey:r,isReplay:i}){if(!e||!e.length)return;this._selectedPairKey=r;let a=e.map(e=>e.id).join(`,`);a!==this._serverIdsKey&&(this._serverIdsKey=a,this._buildTableSkeleton(e));let o=this.container.querySelector(`#matrix-mode-badge`);o&&(o.textContent=i?`REPLAY`:`24H SPARK`,o.classList.toggle(`is-replay`,!!i));for(let i of e)for(let a of e){if(i.id===a.id)continue;let e=`${i.id}->${a.id}`,o=this.container.querySelector(`[data-pair-key="${e}"]`);if(!o)continue;let s=t?t[e]:null,c=s?s.status:`unknown`,l=A(c);o.className=`mx-cell-btn status-${c}${r===e?` is-selected`:``}`,o.style.setProperty(`--cell-accent`,l.color);let u=o.querySelector(`.mx-icon`),d=o.querySelector(`.mx-loss`),f=o.querySelector(`.mx-rtt`),p=o.querySelector(`.mx-spark-wrap`);u.textContent=l.icon,u.style.color=l.color;let m=this._prevValues.get(e)||{rtt:null,loss:null};if(!s)d.textContent=`—`,f.textContent=`wait`,o.title=`${i.id} → ${a.id}: awaiting measurement`;else if(c===`error`)d.textContent=`ERR`,f.textContent=`failed`,o.title=`${i.id} → ${a.id}: ${s.error_msg||`measurement error`}`;else{let t=typeof s.dest_loss==`number`?s.dest_loss:0,n=typeof s.dest_avg_rtt==`number`?s.dest_avg_rtt:null;this._animateNumber(d,m.loss,t,`%`,t%1==0?0:1),n===null?f.textContent=`—`:this._animateNumber(f,m.rtt,n,`ms`,0),this._prevValues.set(e,{rtt:n,loss:t});let r=s.primary_transit_name?` via ${s.primary_transit_name}`:``;o.title=`${i.id} (${i.city}) → ${a.id} (${a.city}): ${l.longLabel} · ${t}% loss · ${n===null?`unreachable`:`${n.toFixed(1)} ms`}${r}`}p.innerHTML=Ce(n&&n[e]||[])}}_buildTableSkeleton(e){let t=e.map(e=>`<th scope="col"><button type="button" class="mx-srv-hdr" data-server-id="${N(e.id)}" title="${N(e.city)}, ${N(e.country)}">${N(e.name)}</button></th>`).join(``),n=e.map(t=>{let n=e.map(e=>t.id===e.id?`<td class="mx-diag" aria-hidden="true"><span>·</span></td>`:`
              <td>
                <button type="button" class="mx-cell-btn status-unknown" data-pair-key="${N(`${t.id}->${e.id}`)}" aria-label="${N(t.name)} to ${N(e.name)}">
                  <div class="mx-top">
                    <span class="mx-icon" aria-hidden="true">○</span>
                    <span class="mx-loss mono">—</span>
                  </div>
                  <div class="mx-rtt mono">wait</div>
                  <div class="mx-spark-wrap"></div>
                </button>
              </td>
            `).join(``);return`
          <tr>
            <th scope="row">
              <button type="button" class="mx-srv-hdr" data-server-id="${N(t.id)}" title="${N(t.city)}, ${N(t.country)}">${N(t.name)}</button>
            </th>
            ${n}
          </tr>
        `}).join(``);this.container.innerHTML=`
      <div class="mx-header">
        <div class="mx-title-wrap">
          <span class="mx-title">PATH MATRIX</span>
          <span class="mx-badge" id="matrix-mode-badge">24H SPARK</span>
        </div>
        <button type="button" id="btn-matrix-toggle" class="mx-collapse-btn" aria-expanded="true">Hide</button>
      </div>
      <div class="mx-body">
        <table class="mx-table">
          <caption class="sr-only">Directed packet loss and latency matrix between servers (rows are source, columns are destination)</caption>
          <thead>
            <tr>
              <th scope="col" class="mx-corner">src \ dst</th>
              ${t}
            </tr>
          </thead>
          <tbody>
            ${n}
          </tbody>
        </table>
      </div>
    `}};function Te(e,t){let n=[];for(let r of e){let e=r.primary_asn||null,i=e?j(e,t):{asn:null,name:`No Reply (*)`,color:`#6b7280`},a=n[n.length-1];a&&a.asn===e||a&&e===null&&r.is_silent&&a.hops.length>0?a.hops.push(r):n.push({asn:e,name:r.carrier_name||i.name,color:i.color,hops:[r]})}return n}function Ee(e){if(e.is_silent)return`*`;let t=e.resolvedHostname||e.resolvedAddress||`?`;if(t===`_gateway`)return`_gateway`;if(t.includes(`.`)){let e=t.split(`.`);return e[0].length>=4?e[0]:e.slice(0,2).join(`.`)}return t}function De(e,t){if(!e.length)return``;let n=Math.max(460,72+(e.length-1)*66),r=new Map;e.forEach((e,t)=>{r.set(e.hop,36+t*66)});let i=[],a=[];for(let e of t){let t=r.get(e.hops[0].hop),n=r.get(e.hops[e.hops.length-1].hop),a=Math.max(60,n-t+48),o=(t+n)/2-a/2,s=e.asn?`${e.name} · AS${e.asn}`:e.name,c=Math.floor((a-10)/5.9),l=s.length<=c?s:e.name.length<=c?e.name:c>=4?`${e.name.slice(0,c-1).trimEnd()}…`:``;i.push(`
      <g class="sw-band">
        <title>${N(s)}</title>
        <rect x="${o}" y="8" width="${a}" height="22" rx="5"
          fill="${M(e.color,.16)}" stroke="${M(e.color,.55)}" stroke-width="1"/>
        <text x="${o+a/2}" y="22.5" text-anchor="middle"
          fill="${e.color}" font-size="10" font-weight="600" font-family="var(--font-sans)">
          ${N(l)}
        </text>
      </g>
    `)}for(let n=0;n<e.length-1;n++){let i=e[n],o=e[n+1],s=r.get(i.hop),c=r.get(o.hop),l=t.find(e=>e.hops.includes(o))||t[0],u=i.is_silent||o.is_silent,d=o.loss_class===`culprit`||o.loss_class===`propagated`,f=d?`#f87171`:u?`rgba(195,194,183,0.32)`:l.color,p=u?`stroke-dasharray="4 4"`:d?`stroke-dasharray="6 3"`:``;a.push(`
      <line x1="${s}" y1="56" x2="${c}" y2="56"
        stroke="${f}" stroke-width="${u?2:3.2}" stroke-linecap="round"
        ${p} class="sw-track-seg" style="--seg-idx:${n}"/>
    `)}let o=e.map((e,n)=>{let i=r.get(e.hop),a=t.find(t=>t.hops.includes(e))||{color:`#7e889b`},o=e.loss_class===`culprit`,s=e.loss_class===`rate_limited`,c=e.is_silent,l=e.loss_class===`propagated`,u=o?`#f87171`:c?`#6b7280`:l?`#fb923c`:a.color,d=o?8.5:c?5:6.5,f=c?`#0d111a`:e.located?u:`#0b0e17`,p=o?`#ff8b8b`:u,m=e.located&&!c?2:2.5,h=c?`stroke-dasharray="2.5 2"`:``,g=e.stats&&e.stats.avg!==null?`${Math.round(e.stats.avg)}ms`:c?`*`:`—`,_=Ee(e),v=e.city?e.city:e.located?``:`interp`,y=``;o?y=`
          <circle cx="${i}" cy="56" r="14" fill="none" stroke="#f87171" stroke-width="1.6" class="sw-culprit-ring"/>
          <text x="${i}" y="42" text-anchor="middle" fill="#ff7676" font-size="8.5" font-weight="700">CULPRIT</text>
        `:s?y=`
          <text x="${i}" y="44" text-anchor="middle" fill="#8b91a0" font-size="8.5" font-family="var(--font-mono)">${Math.round(e.stats.loss)}% RL</text>
        `:l&&e.stats&&e.stats.loss>0&&(y=`
          <text x="${i}" y="44" text-anchor="middle" fill="#fb923c" font-size="8.5" font-family="var(--font-mono)">${Math.round(e.stats.loss)}%</text>
        `);let b=`Hop ${e.hop}: ${e.resolvedHostname||e.resolvedAddress||`*`} | ${g} | ${e.loss_reason||`${e.stats.loss}% loss`}`;return`
        <g class="sw-node ${o?`is-culprit`:``} ${s?`is-rl`:``} ${c?`is-silent`:``}"
           data-hop-num="${e.hop}" style="--node-idx:${n}" tabindex="0" role="button" aria-label="${N(b)}">
          <title>${N(b)}</title>
          <circle cx="${i}" cy="56" r="16" fill="transparent" class="sw-hit"/>
          ${y}
          <circle cx="${i}" cy="56" r="${d}" fill="${f}" stroke="${p}" stroke-width="${m}" ${h} class="sw-dot"/>
          <text x="${i}" y="76" text-anchor="middle" fill="#f0efec" font-size="9.5" font-family="var(--font-mono)">#${e.hop}</text>
          <text x="${i}" y="88" text-anchor="middle" fill="#c3c2b7" font-size="9" font-family="var(--font-mono)">${N(g)}</text>
          <text x="${i}" y="100" text-anchor="middle" fill="${e.located?`#9fb5d6`:`#6c7587`}" font-size="8.5" font-style="${e.located?`normal`:`italic`}">${N(v.slice(0,10))}</text>
          <text x="${i}" y="112" text-anchor="middle" fill="#7c8496" font-size="8" font-family="var(--font-mono)">${N(_.slice(0,11))}</text>
        </g>
      `}).join(``);return`
    <div class="subway-wrap" id="subway-scroll">
      <svg class="subway-svg" width="${n}" height="124" viewBox="0 0 ${n} 124" role="img" aria-label="Hop-by-hop carrier subway diagram">
        ${i.join(``)}
        <g class="sw-track-group" style="--track-len:${n}">${a.join(``)}</g>
        ${o}
      </svg>
    </div>
    <div class="subway-legend">
      <span><i class="leg-dot solid"></i> Located hop</span>
      <span><i class="leg-dot hollow"></i> Interpolated</span>
      <span><i class="leg-dot dashed"></i> Silent (*)</span>
      <span><i class="leg-rl">RL</i> ICMP rate-limited (benign)</span>
    </div>
  `}function Oe({pairKey:e,srcServer:t,dstServer:n,measurement:r,loading:i,carriers:a,routeMode:o,isReplay:s,reverseMeasurement:c}){let l=`${n.id}->${t.id}`,u=r?r.status:`unknown`,d=A(u),f=r&&Array.isArray(r.hops)?r.hops:[],p=Te(f,a),m=!r||u===`error`?`—`:`${Number(r.dest_loss??0).toFixed(r.dest_loss%1==0?0:1)}%`,h=r&&typeof r.dest_avg_rtt==`number`?`${r.dest_avg_rtt.toFixed(1)} ms`:`—`,g=r&&typeof r.dest_min_rtt==`number`&&typeof r.dest_max_rtt==`number`?`${r.dest_min_rtt.toFixed(1)}–${r.dest_max_rtt.toFixed(1)} ms`:null,_=r&&typeof r.dest_jitter==`number`?`±${r.dest_jitter.toFixed(1)} ms`:null,v=c&&c.status!==`error`?`${Number(c.dest_loss??0).toFixed(0)}% · ${typeof c.dest_avg_rtt==`number`?`${c.dest_avg_rtt.toFixed(0)}ms`:`—`}`:``,y=Math.max(1,...f.map(e=>e.stats&&typeof e.stats.avg==`number`?e.stats.avg:0)),b=``;if(i)b=`
      <div class="bgp-loading">
        <span class="inline-spinner"></span> Loading hop-by-hop trace for ${N(t.name)} → ${N(n.name)}…
      </div>
    `;else if(!r)b=`
      <div class="empty-panel-note">
        No measurement recorded yet for <strong>${N(t.name)} → ${N(n.name)}</strong>.
        Click <strong>Refresh now</strong> in the top bar to run a live MTR cycle.
      </div>
    `;else if(u===`error`)b=`
      <div class="panel-error">
        <strong>Measurement failed</strong><br/>
        ${N(r.error_msg||`Unknown Globalping error`)}
      </div>
    `;else{let e=r.probe||{},t=e.city?`${e.city}${e.country?`, ${e.country}`:``} · ${e.network||``}${e.asn?` (AS${e.asn})`:``}`:`Globalping probe`,n=r.route_changed?`
        <div class="route-shift-note">
          <span aria-hidden="true">⇄</span>
          Primary transit changed from <strong>${N(r.previous_transit_name||`AS${r.previous_transit_asn}`)}</strong>
          to <strong>${N(r.primary_transit_name||`AS${r.primary_transit_asn}`)}</strong>
        </div>
      `:``,i=r.culprit_hop_idx?`
        <div class="culprit-callout">
          <span class="cc-icon" aria-hidden="true">✕</span>
          <div>
            <strong>Likely culprit: Hop ${r.culprit_hop_idx}</strong>
            <code class="mono">${N(r.culprit_label||``)}</code>
            ${r.culprit_carrier?`<span class="cc-carrier">(${N(r.culprit_carrier)}${r.culprit_asn?` AS${r.culprit_asn}`:``})</span>`:``}
          </div>
        </div>
      `:``,a=p.map(e=>`<div class="hop-band-group">${`
          <div class="hop-band-hdr" style="--band-color:${e.color}">
            <span class="band-swatch" style="background:${e.color}"></span>
            <span class="band-name">${N(e.name)}</span>
            ${e.asn?`<span class="band-asn mono">AS${e.asn}</span>`:``}
            <span class="band-count">${e.hops.length} ${e.hops.length===1?`hop`:`hops`}</span>
          </div>
        `}${e.hops.map(t=>{let n=t.loss_class===`culprit`,r=t.loss_class===`rate_limited`,i=t.is_silent,a=t.loss_class===`propagated`,o=i?`* (no ICMP reply — gap)`:t.resolvedHostname||t.resolvedAddress||`?`,s=!i&&t.resolvedHostname&&t.resolvedAddress&&t.resolvedHostname!==t.resolvedAddress?t.resolvedAddress:``,c=t.stats&&typeof t.stats.avg==`number`?t.stats.avg:null,l=c===null?0:Math.max(3,Math.min(100,c/y*100)),u=``;u=i?`<span class="loss-silent" title="${N(t.loss_reason)}">gap</span>`:r?`
                <span class="loss-rl" title="${N(t.loss_reason)}">
                  ${Number(t.stats.loss).toFixed(0)}% <span class="rl-tag">RL</span>
                </span>
              `:n?`
                <span class="loss-culprit" title="${N(t.loss_reason)}">
                  ✕ ${Number(t.stats.loss).toFixed(0)}%
                </span>
              `:a&&t.stats.loss>0?`
                <span class="loss-prop" title="${N(t.loss_reason)}">
                  ◆ ${Number(t.stats.loss).toFixed(0)}%
                </span>
              `:`<span class="loss-ok">0%</span>`;let d=t.located?`<span class="loc-pill is-located">${N(t.city||`Located`)}</span>`:`<span class="loc-pill is-interp" title="Coordinates interpolated along great circle between adjacent known hops">○ interp</span>`;return`
              <div class="hop-row ${n?`is-culprit`:``} ${r?`is-rl`:``} ${i?`is-silent`:``}"
                   data-hop-num="${t.hop}" style="--band-color:${e.color}">
                <span class="hop-idx mono">${t.hop}</span>
                <div class="hop-host-col">
                  <div class="hop-host mono" title="${N(o)}">${N(o)}</div>
                  <div class="hop-sub">
                    ${s?`<span class="mono hop-ip">${N(s)}</span>`:``}
                    ${d}
                    ${n?`<span class="culprit-pill">LIKELY CULPRIT</span>`:``}
                    ${r?`<span class="rl-explain" title="${N(t.loss_reason)}">ICMP rate limit</span>`:``}
                  </div>
                </div>
                <div class="hop-rtt-col">
                  <span class="mono">${c===null?`—`:`${c.toFixed(1)} ms`}</span>
                  <div class="rtt-bar-track">
                    <div class="rtt-bar-fill" style="width:${l.toFixed(1)}%;background:${i?`transparent`:e.color}"></div>
                  </div>
                </div>
                <div class="hop-loss-col mono">
                  ${u}
                </div>
              </div>
            `}).join(``)}</div>`).join(``);b=`
      <div class="path-kpis">
        <div class="kpi-box">
          <span class="kv-label">STATUS</span>
          <span class="status-badge" style="color:${d.color};background:${d.bg};border-color:${d.border}">
            ${d.icon} ${d.longLabel}
          </span>
        </div>
        <div class="kpi-box">
          <span class="kv-label">DEST LOSS</span>
          <span class="kpi-num mono" style="color:${r.dest_loss>0?d.color:`#f0efec`}">${m}</span>
        </div>
        <div class="kpi-box">
          <span class="kv-label">AVG RTT</span>
          <span class="kpi-num mono">${h}</span>
          ${g?`<span class="kpi-sub mono">${g}${_?` · ${_}`:``}</span>`:``}
        </div>
      </div>

      <div class="probe-line">
        <span class="kv-label">PROBE</span>
        <span class="probe-text">${N(t)}</span>
        ${e.selector?`<code class="probe-sel">${N(e.selector)}</code>`:``}
      </div>

      ${n}
      ${i}

      <section class="drawer-section subway-section">
        <div class="section-hdr">
          <span>SUBWAY ROUTE (${f.length} HOPS)</span>
          <span class="section-meta">Hover hop to highlight on globe</span>
        </div>
        ${De(f,p)}
      </section>

      <section class="drawer-section hops-table-section">
        <div class="section-hdr">
          <span>HOP-BY-HOP DETAIL</span>
          <span class="section-meta mono">${P(r.ts,!0)}</span>
        </div>
        <div class="hop-list-header">
          <span>#</span>
          <span>ROUTER / LOCATION</span>
          <span class="right">AVG RTT</span>
          <span class="right">LOSS</span>
        </div>
        <div class="hop-bands-container">
          ${a}
        </div>
      </section>
    `}return`
    <div class="drawer-header">
      <div class="drawer-title-group">
        <div class="drawer-eyebrow">
          <span class="eyebrow-tag">DIRECTED PATH</span>
          ${s?`<span class="pill pill-warn">REPLAY</span>`:``}
        </div>
        <h2 class="drawer-title">
          <span class="srv-code">${N(t.name)}</span>
          <span class="pair-arrow">→</span>
          <span class="srv-code">${N(n.name)}</span>
          <span class="srv-city-title">${N(t.city)} → ${N(n.city)}</span>
        </h2>
        <div class="path-quick-actions">
          <button type="button" class="mini-btn" data-open-pair="${N(l)}" title="Inspect reverse direction to check asymmetry">
            ⇄ Flip ${N(n.name)}→${N(t.name)}${v?` (${v})`:``}
          </button>
          <button type="button" class="mini-btn ${o?`is-active`:``}" data-toggle-route-mode>
            ${o?`● Globe: Real Route`:`○ Show All Real Routes`}
          </button>
        </div>
      </div>
      <button type="button" class="drawer-close" data-close-drawer aria-label="Close path panel">✕</button>
    </div>

    <div class="drawer-scroll">
      ${b}
    </div>
  `}function ke(e){if(e==null)return``;if(e<60)return`~${e}m behind UTC`;let t=Math.floor(e/60),n=e%60;return n>0?`~${t}h ${n}m behind UTC`:`~${t}h behind UTC`}function Ae({server:e,servers:t,pairData:n,bgp:r,bgpLoading:i,carriers:a,isReplay:o}){let s=[],c=[];for(let r of t){if(r.id===e.id)continue;let t=`${e.id}->${r.id}`,i=`${r.id}->${e.id}`;s.push({key:t,peer:r,data:n?n[t]:null}),c.push({key:i,peer:r,data:n?n[i]:null})}let l=`good`,u=!1;for(let e of[...c,...s])e.data&&e.data.status!==`error`&&(u=!0,A(e.data.status).severity>A(l).severity&&(l=e.data.status));let d=A(u?l:`unknown`),f=``;if(i&&!r)f=`
      <div class="bgp-loading">
        <span class="inline-spinner"></span> Fetching RIPEstat BGP status for ${N(e.prefix||e.ip)}…
      </div>
    `;else if(r){let t=r.visibility||{},n=t.ris_peers_seeing,o=t.total_ris_peers,s=n&&o?(n/o*100).toFixed(1):null,c=r.announced===!0?`<span class="pill pill-good">● ANNOUNCED</span>`:r.announced===!1?`<span class="pill pill-crit">✕ NOT ANNOUNCED</span>`:`<span class="pill pill-muted">○ UNKNOWN</span>`,l=r.upstreams&&r.upstreams.length?r.upstreams.map(e=>{let t=j(e.asn,a);return`
                <span class="carrier-chip" style="border-color:${M(t.color,.55)};background:${M(t.color,.14)}" title="${N(e.holder||e.name)} (power: ${e.power})">
                  <span class="chip-dot" style="background:${t.color}"></span>
                  <span>${N(e.name)}</span>
                  <span class="chip-asn mono">AS${e.asn}</span>
                </span>
              `}).join(``):`<span class="muted">No upstream data</span>`,u=r.bgp_updates||{},d=``;if(u.query_starttime&&u.query_endtime){let e=ke(u.lag_minutes);d=`
        <div class="bgp-updates-box">
          <div class="bgp-updates-top">
            <span class="bgp-label">BGP UPDATES (DELAYED WINDOW)</span>
            <span class="mono bgp-upd-count">${u.nr_updates??0} total (${u.announcements??0}A / ${u.withdrawals??0}W)</span>
          </div>
          <div class="bgp-updates-window mono">
            ${P(u.query_starttime,!0)} → ${P(u.query_endtime,!0)}
          </div>
          ${u.last_update_ts?`<div class="bgp-updates-last">Last observed update in window: <span class="mono">${P(u.last_update_ts,!0)}</span></div>`:`<div class="bgp-updates-last">No updates observed in returned window.</div>`}
          <div class="lag-caveat" role="note">
            <span class="lag-icon" aria-hidden="true">⏱</span>
            <span>RIPEstat data lags real time${e?` (${e})`:``}; window shown above is historical coverage, not live BGP.</span>
          </div>
        </div>
      `}else u.error&&(d=`<div class="lag-caveat">RIPEstat bgp-updates unavailable: ${N(u.error)}</div>`);let p=j(r.origin_asn,a);f=`
      ${r.error?`<div class="panel-error">${N(r.error)}</div>`:``}
      ${r.stale_warning?`<div class="lag-caveat">${N(r.stale_warning)}</div>`:``}
      <div class="kv-grid">
        <div class="kv-item">
          <span class="kv-label">PREFIX</span>
          <span class="kv-val mono">${N(r.prefix||e.prefix||`—`)}</span>
        </div>
        <div class="kv-item">
          <span class="kv-label">ROUTING</span>
          <span class="kv-val">${c}</span>
        </div>
        <div class="kv-item kv-full">
          <span class="kv-label">ORIGIN AS</span>
          <span class="kv-val">
            <span class="chip-dot inline-dot" style="background:${p.color}"></span>
            <strong class="mono">AS${N(r.origin_asn??e.asn)}</strong>
            <span class="secondary">${N(r.origin_holder||e.network)}</span>
          </span>
        </div>
      </div>

      ${n!=null&&o?`
        <div class="vis-block">
          <div class="vis-top">
            <span class="kv-label">RIS VISIBILITY</span>
            <span class="mono">${n} / ${o} peers (${s}%)</span>
          </div>
          <div class="vis-bar-track" role="progressbar" aria-valuenow="${n}" aria-valuemin="0" aria-valuemax="${o}">
            <div class="vis-bar-fill" style="width:${Math.min(100,n/o*100).toFixed(1)}%"></div>
          </div>
          <div class="vis-dates">
            <span>First seen: <span class="mono">${P(r.first_seen,!0)}</span></span>
            <span>Last seen: <span class="mono">${P(r.last_seen,!0)}</span></span>
          </div>
        </div>
      `:``}

      <div class="upstreams-block">
        <span class="kv-label">UPSTREAM ASNs (RIPE RIS)</span>
        <div class="carrier-chip-list">${l}</div>
      </div>

      ${d}

      <div class="bgp-actions">
        <a class="hud-btn hud-btn-outline" href="${N(r.arelion_lg_url)}" target="_blank" rel="noopener noreferrer">
          Arelion Looking Glass ↗
        </a>
        <button type="button" class="hud-btn hud-btn-ghost" data-refresh-bgp="${N(e.id)}">
          ${i?`Refreshing…`:`Refresh BGP`}
        </button>
      </div>
    `}let p=(e,t)=>`
    <div class="path-list-group">
      <div class="section-subhdr">${t}</div>
      <div class="path-list">
        ${e.map(({key:e,peer:t,data:n})=>{let r=n?n.status:`unknown`,i=A(r),o=n?r===`error`?`ERR`:`${Number(n.dest_loss??0).toFixed(n.dest_loss%1==0?0:1)}%`:`—`,s=n&&typeof n.dest_avg_rtt==`number`?`${n.dest_avg_rtt.toFixed(1)} ms`:`—`,c=n&&n.primary_transit_asn?j(n.primary_transit_asn,a):null;return`
              <button type="button" class="srv-path-row" data-open-pair="${N(e)}">
                <div class="spr-left">
                  <span class="spr-pair mono">${N(e.replace(`->`,` → `))}</span>
                  <span class="spr-city">${N(t.city)}</span>
                </div>
                <div class="spr-mid">
                  ${c?`<span class="mini-carrier" style="color:${c.color}">● ${N(c.name)}</span>`:``}
                </div>
                <div class="spr-right">
                  <span class="spr-rtt mono">${s}</span>
                  <span class="status-badge" style="color:${i.color};background:${i.bg};border-color:${i.border}">
                    ${i.icon} ${o}
                  </span>
                </div>
              </button>
            `}).join(``)}
      </div>
    </div>
  `;return`
    <div class="drawer-header">
      <div class="drawer-title-group">
        <div class="drawer-eyebrow">
          <span class="status-badge" style="color:${d.color};background:${d.bg};border-color:${d.border}">
            ${d.icon} ${d.longLabel.toUpperCase()}
          </span>
          ${o?`<span class="pill pill-warn">REPLAY SNAPSHOT</span>`:``}
        </div>
        <h2 class="drawer-title">
          <span class="srv-code">${N(e.name)}</span>
          <span class="srv-city-title">${N(e.city)}, ${N(e.country)}</span>
        </h2>
        <div class="drawer-subtitle mono">${N(e.host)} · ${N(e.ip)}</div>
      </div>
      <button type="button" class="drawer-close" data-close-drawer aria-label="Close server panel">✕</button>
    </div>

    <div class="drawer-scroll">
      <section class="drawer-section">
        <div class="kv-grid">
          <div class="kv-item kv-full">
            <span class="kv-label">NETWORK</span>
            <span class="kv-val">${N(e.network)}</span>
          </div>
          <div class="kv-item kv-full">
            <span class="kv-label">LOCATION NOTE</span>
            <span class="kv-val secondary">${N(e.location_note||`${e.city}, ${e.country}`)}</span>
          </div>
        </div>
      </section>

      <section class="drawer-section">
        <div class="section-hdr">
          <span>BGP &amp; ROUTING STATUS</span>
          <span class="section-meta mono">${r&&r.fetched_at?`Cached ${P(r.fetched_at)}`:`RIPEstat`}</span>
        </div>
        ${f}
      </section>

      <section class="drawer-section">
        <div class="section-hdr">
          <span>${o?`REPLAY PATHS`:`LIVE PATHS`}</span>
          <span class="section-meta">Click any path for hops</span>
        </div>
        ${p(c,`Inbound → ${e.name}`)}
        ${p(s,`Outbound ${e.name} →`)}
      </section>
    </div>
  `}var je=class{constructor(e,t={}){this.container=e,this.callbacks=t,this.snapshots=[],this.currentIndex=-1,this.isPlaying=!1,this._playTimer=null,this._dragging=!1,this._renderSkeleton(),this._bindEvents()}_renderSkeleton(){this.container.innerHTML=`
      <div class="tl-left">
        <button type="button" id="tl-play-btn" class="tl-icon-btn" title="Play/Pause 24h replay" aria-label="Play 24-hour replay" disabled>
          ▶
        </button>
        <div class="tl-mode-wrap">
          <span class="tl-live-pill is-live" id="tl-mode-pill">● LIVE</span>
          <button type="button" id="tl-back-live" class="tl-back-live" hidden>Return to live</button>
        </div>
      </div>

      <div class="tl-center">
        <div class="tl-meta-row">
          <span class="tl-window-label">24H TIMELINE</span>
          <span class="tl-current-ts mono" id="tl-current-ts">Awaiting first cycle</span>
          <span class="tl-incident-hint" id="tl-incident-hint"></span>
        </div>

        <div class="tl-track-outer" id="tl-track" role="slider" tabindex="0"
             aria-label="24-hour history timeline scrubber" aria-valuemin="0" aria-valuemax="0" aria-valuenow="0">
          <div class="tl-track-bg"></div>
          <svg class="tl-loss-curve" id="tl-loss-curve" preserveAspectRatio="none" viewBox="0 0 1000 22" aria-hidden="true"></svg>
          <div class="tl-ticks" id="tl-ticks"></div>
          <div class="tl-progress" id="tl-progress"></div>
          <div class="tl-thumb" id="tl-thumb"></div>
        </div>
      </div>

      <div class="tl-right">
        <div class="tl-summary" id="tl-summary">
          <span class="tl-sum-main mono">—</span>
          <span class="tl-sum-sub">0 cycles</span>
        </div>
      </div>
    `,this.els={playBtn:this.container.querySelector(`#tl-play-btn`),modePill:this.container.querySelector(`#tl-mode-pill`),backLiveBtn:this.container.querySelector(`#tl-back-live`),currentTs:this.container.querySelector(`#tl-current-ts`),incidentHint:this.container.querySelector(`#tl-incident-hint`),track:this.container.querySelector(`#tl-track`),lossCurve:this.container.querySelector(`#tl-loss-curve`),ticks:this.container.querySelector(`#tl-ticks`),progress:this.container.querySelector(`#tl-progress`),thumb:this.container.querySelector(`#tl-thumb`),summary:this.container.querySelector(`#tl-summary`)}}_bindEvents(){this.els.backLiveBtn.addEventListener(`click`,()=>{this.stopPlayback(),this.selectIndex(-1)}),this.els.playBtn.addEventListener(`click`,()=>{this.snapshots.length<2||(this.isPlaying?this.stopPlayback():this.startPlayback())});let e=e=>{if(!this.snapshots.length)return;let t=this.els.track.getBoundingClientRect(),n=Math.max(0,Math.min(1,(e-t.left)/Math.max(1,t.width))),r=Math.round(n*(this.snapshots.length-1));r===this.snapshots.length-1?this.selectIndex(-1):this.selectIndex(r)};this.els.track.addEventListener(`pointerdown`,t=>{this.snapshots.length&&(this.stopPlayback(),this._dragging=!0,this.els.track.setPointerCapture(t.pointerId),e(t.clientX))}),this.els.track.addEventListener(`pointermove`,t=>{this._dragging&&e(t.clientX)});let t=()=>{this._dragging=!1};this.els.track.addEventListener(`pointerup`,t),this.els.track.addEventListener(`pointercancel`,t),this.els.track.addEventListener(`keydown`,e=>{if(!this.snapshots.length)return;let t=this.currentIndex===-1?this.snapshots.length-1:this.currentIndex;if(e.key===`ArrowLeft`)e.preventDefault(),this.stopPlayback(),this.selectIndex(Math.max(0,t-1));else if(e.key===`ArrowRight`){e.preventDefault(),this.stopPlayback();let n=Math.min(this.snapshots.length-1,t+1);this.selectIndex(n===this.snapshots.length-1?-1:n)}else e.key===`End`?(e.preventDefault(),this.stopPlayback(),this.selectIndex(-1)):e.key===`Home`&&(e.preventDefault(),this.stopPlayback(),this.selectIndex(0))})}startPlayback(){this.snapshots.length<2||(this.isPlaying=!0,this.els.playBtn.textContent=`❚❚`,this.els.playBtn.setAttribute(`aria-label`,`Pause replay`),(this.currentIndex===-1||this.currentIndex>=this.snapshots.length-1)&&this.selectIndex(0),this._playTimer=setInterval(()=>{let e=(this.currentIndex===-1?0:this.currentIndex)+1;e>=this.snapshots.length-1?(this.selectIndex(-1),this.stopPlayback()):this.selectIndex(e)},1150))}stopPlayback(){this.isPlaying=!1,this._playTimer&&=(clearInterval(this._playTimer),null),this.els.playBtn.textContent=`▶`,this.els.playBtn.setAttribute(`aria-label`,`Play 24-hour replay`)}selectIndex(e){if(this.currentIndex=e,this._renderPosition(),this.callbacks.onScrub){let t=e>=0&&e<this.snapshots.length?this.snapshots[e]:null;this.callbacks.onScrub(t,e)}}update(e=[],t=this.currentIndex){this.snapshots=e||[],this.currentIndex=t>=this.snapshots.length?-1:t,this.els.playBtn.disabled=this.snapshots.length<2,this.els.track.setAttribute(`aria-valuemax`,String(Math.max(0,this.snapshots.length-1))),this._renderTicksAndCurve(),this._renderPosition()}_renderTicksAndCurve(){let e=this.snapshots.length;if(!e){this.els.ticks.innerHTML=``,this.els.lossCurve.innerHTML=``,this.els.incidentHint.textContent=``;return}let t=0,n=[],r=[];for(let i=0;i<e;i++){let a=this.snapshots[i],o=e===1?100:i/(e-1)*100,s=Number(a.max_loss||0),c=21-Math.min(100,s)*.19;r.push(`${(o*10).toFixed(1)},${c.toFixed(1)}`),a.incident?(t+=1,n.push(`<span class="tl-tick is-incident" style="left:${o.toFixed(2)}%" title="${N(`${P(a.ts,!0)} — lasting packet loss on a path, max ${s.toFixed(1)}%`)}"></span>`)):n.push(`<span class="tl-tick" style="left:${o.toFixed(2)}%" title="${N(`${P(a.ts,!0)} — max ${s.toFixed(1)}% loss`)}"></span>`)}if(this.els.ticks.innerHTML=n.join(``),e>=2){let e=t>0?`rgba(251,191,36,0.6)`:`rgba(57,135,229,0.5)`,n=t>0?`rgba(251,191,36,0.12)`:`rgba(57,135,229,0.1)`,i=`0,22 ${r.join(` `)} 1000,22`;this.els.lossCurve.innerHTML=`
        <polygon points="${i}" fill="${n}" />
        <polyline points="${r.join(` `)}" fill="none" stroke="${e}" stroke-width="1.6" />
      `}else this.els.lossCurve.innerHTML=``;this.els.incidentHint.innerHTML=t>0?`<span class="tl-inc-dot"></span> ${t} ${t===1?`round`:`rounds`} with lasting path loss`:`<span class="muted">No lasting path loss in 24h</span>`}_renderPosition(){let e=this.snapshots.length,t=this.currentIndex===-1;if(this.els.modePill.textContent=t?`● LIVE`:`↺ REPLAY`,this.els.modePill.classList.toggle(`is-live`,t),this.els.modePill.classList.toggle(`is-replay`,!t),this.els.backLiveBtn.hidden=t,!e){this.els.progress.style.width=`0%`,this.els.thumb.style.left=`0%`,this.els.currentTs.textContent=`No history yet — run Refresh now`,this.els.summary.innerHTML=`<span class="tl-sum-main mono">—</span><span class="tl-sum-sub">0 cycles</span>`;return}let n=t?e-1:this.currentIndex,r=e===1?100:n/(e-1)*100;this.els.progress.style.width=`${r.toFixed(2)}%`,this.els.thumb.style.left=`${r.toFixed(2)}%`,this.els.track.setAttribute(`aria-valuenow`,String(n));let i=this.snapshots[n];this.els.currentTs.textContent=`${P(i.ts,!0)} · Cycle #${i.cycle_id}`;let a=Object.values(i.pairs||{}),o=a.filter(e=>e.status===`good`).length,s=a.filter(e=>[`warning`,`serious`,`critical`].includes(e.status)).length,c=i.incident?`is-crit`:s>0?`is-warn`:`is-ok`;this.els.summary.innerHTML=`
      <span class="tl-sum-main mono ${c}">${o}/${a.length} OK${i.max_loss>0?` · max ${i.max_loss}%`:``}</span>
      <span class="tl-sum-sub">${e} ${e===1?`cycle`:`cycles`} (24h)</span>
    `}},Me=class{constructor(){this.state=null,this.bgpByServer={},this.bgpLoading={},this.historicalMeasCache=new Map,this.routeMode=!1,this.selectedServerId=null,this.selectedPairKey=null,this.selectedPairLoading=!1,this.replayIndex=-1,this.replaySnapshot=null,this._pollTimer=null,this._wasRunning=!1,this.simple=x||(C?C===`simple`:localStorage.getItem(`netviz-view`)!==`details`),this.network=null,this.simpleSelection=null,this._networkTimer=null,this.simpleCardEl=document.getElementById(`simple-card`),this.simpleDetailEl=document.getElementById(`simple-detail`),this.viewSwitchEl=document.getElementById(`view-switch`),this.drawerEl=document.getElementById(`inspector-drawer`),this.legendEl=document.getElementById(`carrier-legend`),this.toastRoot=document.getElementById(`toast-root`),this._initComponents(),this._bindGlobalEvents(),this._bindSimpleEvents(),this._applyViewMode(),this._bootstrap(),this._pollNetwork()}_initComponents(){this.globe=new de(document.getElementById(`globe-container`),{onServerClick:e=>this.selectServer(e),onArcClick:(e,t)=>this.selectPair(e,t),onArcHover:e=>this._highlightMatrixPair(e),onHopHover:(e,t)=>this._highlightPanelHop(e,t),onResize:()=>this._layoutSimple()}),this.orbit=new ge(document.getElementById(`orbit-layer`),document.getElementById(`orbit-tip`),{getGlobeCircle:()=>this.globe.getGlobeCircle(),onSelect:e=>this.selectOrbitNode(e),hostEl:document.getElementById(`globe-container`)}),this.header=new xe(document.getElementById(`hud-header`),{onRouteModeChange:e=>this.setRouteMode(e),onIntervalChange:e=>this.handleIntervalChange(e),onRefresh:()=>this.handleManualRefresh(),onResetView:()=>this.resetView(),onSimpleView:()=>this.setSimple(!0)}),this.incidentBanner=new Se(document.getElementById(`incident-banner`),{onInspectPair:e=>this.selectPair(e)}),this.matrix=new we(document.getElementById(`matrix-hud`),{onServerClick:e=>this.selectServer(e),onPairClick:e=>this.selectPair(e),onPairHover:e=>this.globe.setHoveredPair(e)}),this.timeline=new je(document.getElementById(`timeline-hud`),{onScrub:(e,t)=>this.handleTimelineScrub(e,t)})}_bindGlobalEvents(){window.addEventListener(`keydown`,e=>{if(e.key===`Escape`){if(this.simple){this.closeSimpleDetail();return}(this.selectedServerId||this.selectedPairKey)&&this.resetView()}}),this.drawerEl.addEventListener(`click`,e=>{if(e.target.closest(`[data-close-drawer]`)){this.resetView();return}let t=e.target.closest(`[data-open-pair]`);if(t){this.selectPair(t.dataset.openPair);return}let n=e.target.closest(`[data-refresh-bgp]`);n?this._loadServerBgp(n.dataset.refreshBgp,!0):e.target.closest(`[data-toggle-route-mode]`)&&this.setRouteMode(!this.routeMode)}),this.drawerEl.addEventListener(`mouseover`,e=>{let t=e.target.closest(`[data-hop-num]`);if(t&&this.selectedPairKey){let e=parseInt(t.dataset.hopNum,10);Number.isNaN(e)||(this._setPanelHopHighlight(e,!1),this.globe.setHoveredHop(e,this.selectedPairKey))}}),this.drawerEl.addEventListener(`mouseleave`,()=>{this.selectedPairKey&&(this._setPanelHopHighlight(null,!1),this.globe.setHoveredHop(null,this.selectedPairKey))})}_bindSimpleEvents(){this.viewSwitchEl.addEventListener(`click`,()=>this.setSimple(!1)),this.simpleDetailEl.addEventListener(`click`,e=>{e.target.closest(`[data-close-detail]`)&&this.closeSimpleDetail()}),setInterval(()=>{this.simple&&!document.hidden&&this._renderSimpleCard()},3e4)}setSimple(e){x||(this.simple=!!e,localStorage.setItem(`netviz-view`,this.simple?`simple`:`details`),this._applyViewMode(),this._renderAll())}_applyViewMode(){w().classList.toggle(`is-simple`,this.simple),this.selectedServerId=null,this.selectedPairKey=null,this.selectedPairLoading=!1,this.simpleSelection=null,this.globe.setSelection({serverId:null,pairKey:null,measurement:null}),this.simple?(this.routeMode=!1,this.globe.setRouteMode(!1),this._renderSimpleCard(),this.globe.setSimpleMode(!0,this._computeSimpleLayout()),this._layoutSimple()):(w().classList.remove(`is-compact`),this.globe.setSimpleMode(!1)),this.orbit.setVisible(this.simple),this.orbit.setSelected(null),this._renderSimpleDetail()}_computeSimpleLayout(){let e=document.getElementById(`globe-container`),t=e.clientWidth||window.innerWidth,n=e.clientHeight||window.innerHeight,r=x?18:44,i=e.getBoundingClientRect(),a=e=>{let t=e.getBoundingClientRect();return{left:t.left-i.left,right:t.right-i.left,top:t.top-i.top,bottom:t.bottom-i.top,width:t.width}},o=[this.simpleCardEl,this.viewSwitchEl].map(a).filter(e=>e.width>0),s=(e,t,n)=>{let r=Math.max(n.left-e,0,e-n.right),i=Math.max(n.top-t,0,t-n.bottom);return Math.hypot(r,i)},c=(e,i,a)=>a<=Math.min(e,t-e,i,n-i)-r&&o.every(t=>s(e,i,t)>=a+48),l=null,u=n/2;for(let e=Math.min(t,n)/2-r;e>=150&&!l;e-=4)for(let n=t/2;n<=t-r-e;n+=6)if(c(n,u,e)){l={cx:n,cy:u,outer:e};break}let d=a(this.simpleCardEl).bottom+8,f={cx:t/2,cy:(n+d)/2,outer:Math.min((n-d)/2-r,t/2-r)},p=l&&l.outer>=f.outer?l:f,m=Math.max(150,p.outer);return{cx:p.cx,cy:p.cy,walletR:m,minerR:m*.79,globePx:m*.5,scale:Math.min(1.15,Math.max(.5,m/400))}}_layoutSimple(){if(!this.simple)return;let e=this._computeSimpleLayout();w().classList.toggle(`is-compact`,e.globePx<130),this.globe.setSimpleLayout(e),this.orbit.setLayout(e)}async _pollNetwork(){this._networkTimer&&clearTimeout(this._networkTimer);let e=3e4;try{if(!document.hidden||!this.network){let t=await te();t&&t.ready?(this.network=t,this.orbit.setData(t),this.globe.setValidators(this._validatorInfoByServer()),this._renderSimpleCard(),this._renderSimpleDetail(),this._layoutSimple()):(e=2500,this.network||this._renderSimpleCard())}}catch{e=5e3}this._networkTimer=setTimeout(()=>this._pollNetwork(),e)}_validatorInfoByServer(){let e={},t=this.state&&this.state.servers||[],n=this.network&&this.network.validators||[];return t.forEach((t,r)=>{let i=n.find(e=>e.seat===r);i&&(e[t.id]={number:r+1,up:i.up,agreed:i.agreed,of:i.of})}),e}_renderSimpleCard(){this.simpleCardEl.innerHTML=ye(this.network)}_renderSimpleDetail(){let e=this.simple?be(this.simpleSelection):``;this.simpleDetailEl.hidden=!e,this.simpleDetailEl.innerHTML=e}selectOrbitNode(e){x||(this.simpleSelection={type:`node`,node:e},this.orbit.setSelected(e.key),this._renderSimpleDetail())}_selectSimpleValidator(e){if(x)return;let t=this.state&&this.state.servers||[],n=t.find(t=>t.id===e);if(!n)return;let r=this.state&&this.state.latest||{},i=[];for(let n of t){if(n.id===e)continue;let t=[r[`${e}->${n.id}`],r[`${n.id}->${e}`]].filter(e=>e&&e.status!==`error`&&typeof e.dest_avg_rtt==`number`).map(e=>e.dest_avg_rtt);t.length&&i.push({city:z[n.city]||n.city,rtt:t.reduce((e,t)=>e+t,0)/t.length})}this.simpleSelection={type:`validator`,server:n,info:this._validatorInfoByServer()[e]||null,links:i},this.orbit.setSelected(null),this._renderSimpleDetail()}closeSimpleDetail(){this.simpleSelection=null,this.orbit.setSelected(null),this._renderSimpleDetail()}async _bootstrap(){try{await this.refreshState(),E().then(e=>{e&&e.servers&&(Object.assign(this.bgpByServer,e.servers),e.carriers&&this.state&&(this.state.carriers={...this.state.carriers||{},...e.carriers}),this.selectedServerId&&this._renderDrawer())}).catch(()=>{})}catch(e){this.showToast(`Failed to reach backend: ${e.message}`,`error`)}this._schedulePoll()}_schedulePoll(){this._pollTimer&&clearTimeout(this._pollTimer);let e=!x&&!!(this.state&&this.state.scheduler&&this.state.scheduler.running),t=x?6e4:e?1200:15e3;this._pollTimer=setTimeout(async()=>{if(document.hidden)this._schedulePoll();else{try{if(e){let e=await ne();if(this.state&&(this.state.scheduler=e.scheduler,this.state.quota=e.quota),e.scheduler.running)e.scheduler.progress&&e.scheduler.progress.completed>0?await this.refreshState():this._renderAll();else{await this.refreshState();let t=e.scheduler.progress?e.scheduler.progress.errors:0;this.showToast(t>0?`Measurement cycle completed with ${t} probe error(s).`:`Measurement cycle completed (12/12 pairs updated).`,t>0?`warn`:`ok`)}}else await this.refreshState()}catch{}this._schedulePoll()}},t)}async refreshState(){let e=await ee();this.state=e,this.network&&this.globe.setValidators(this._validatorInfoByServer()),this._wasRunning=!!(e.scheduler&&e.scheduler.running),this._renderAll()}_getActivePairData(){if(!this.state)return{};if(this.replayIndex>=0&&this.replaySnapshot){let e={};for(let[t,n]of Object.entries(this.replaySnapshot.pairs||{}))e[t]=this.historicalMeasCache.get(n.id)||n;return e}return this.state.latest||{}}_getActiveIncidents(){return this.state?this.replayIndex>=0&&this.replaySnapshot?this.replaySnapshot.incidents||[]:this.state.incidents||[]:[]}async handleManualRefresh(){if(!b)try{let e=await re();this.state&&e.scheduler&&(this.state.scheduler=e.scheduler),e.started&&this.showToast(`Running 12 MTR measurements via Globalping…`,`info`),this._renderAll(),this._schedulePoll()}catch(e){this.showToast(`Could not start refresh: ${e.message}`,`error`)}}async handleIntervalChange(e){if(!b)try{let t=await ie(e);this.state&&t.scheduler&&(this.state.scheduler=t.scheduler),this._renderAll(),this.showToast(e===0?`Auto-refresh disabled (manual only).`:`Auto-refresh set to every ${e} min.`,`info`)}catch(e){this.showToast(`Failed to update interval: ${e.message}`,`error`)}}setRouteMode(e){this.routeMode=!!e,this.globe.setRouteMode(this.routeMode),this._renderAll()}async selectServer(e){this.state&&(this.simple?this._selectSimpleValidator(e):(this.state.servers||[]).find(t=>t.id===e)&&(this.selectedServerId=e,this.selectedPairKey=null,this.selectedPairLoading=!1,this.globe.setSelection({serverId:e,pairKey:null,measurement:null}),this.globe.flyToServer(e),this._renderAll(),this.bgpByServer[e]||this._loadServerBgp(e,!1)))}async _loadServerBgp(e,t=!1){this.bgpLoading[e]=!0,this._renderDrawer();try{let n=await E(e,t);this.bgpByServer[e]=n,t&&await this.refreshState()}catch(e){this.showToast(`RIPEstat lookup failed: ${e.message}`,`error`)}finally{this.bgpLoading[e]=!1,this._renderDrawer()}}async selectPair(e,t=null){if(this.simple||!this.state||!e||!e.includes(`->`))return;let[n,r]=e.split(`->`),i=this._getActivePairData()[e]||null;if(this.selectedServerId=null,this.selectedPairKey=e,i&&!Array.isArray(i.hops)&&i.id){let t=this.historicalMeasCache.get(i.id);if(t)i=t;else{this.selectedPairLoading=!0,this.globe.setSelection({serverId:null,pairKey:e,measurement:i}),this.globe.flyToPair(n,r,[]),this._renderAll();try{let e=await D(i.id);this.historicalMeasCache.set(i.id,e),i=e}catch(e){this.showToast(`Could not load historical hops: ${e.message}`,`error`)}finally{this.selectedPairLoading=!1}}}this.selectedPairKey===e&&(this.globe.setSelection({serverId:null,pairKey:e,measurement:i}),this.globe.flyToPair(n,r,i&&i.hops?i.hops:[]),this._renderAll(),t&&setTimeout(()=>{this._setPanelHopHighlight(t,!0),this.globe.setHoveredHop(t,e)},280))}async handleTimelineScrub(e,t){if(this.replayIndex=t,this.replaySnapshot=e,this.selectedPairKey&&t>=0&&e&&e.pairs){let t=e.pairs[this.selectedPairKey];if(t&&t.id&&!this.historicalMeasCache.has(t.id)){this._renderAll();try{let e=await D(t.id);this.historicalMeasCache.set(t.id,e)}catch{}}}let n=this._getActivePairData();this.selectedPairKey&&this.globe.setSelection({serverId:null,pairKey:this.selectedPairKey,measurement:n[this.selectedPairKey]||null}),this._renderAll()}resetView(){this.selectedServerId=null,this.selectedPairKey=null,this.selectedPairLoading=!1,this.globe.resetView(),this._renderAll()}_highlightMatrixPair(e){if(document.querySelectorAll(`.mx-cell-btn.is-globe-hover`).forEach(e=>e.classList.remove(`is-globe-hover`)),e){let t=document.querySelector(`.mx-cell-btn[data-pair-key="${e}"]`);t&&t.classList.add(`is-globe-hover`)}}_highlightPanelHop(e,t){this.selectedPairKey&&(t&&t!==this.selectedPairKey||this._setPanelHopHighlight(e,!0))}_setPanelHopHighlight(e,t=!1){if(this.drawerEl.querySelectorAll(`.is-hop-hover`).forEach(e=>e.classList.remove(`is-hop-hover`)),e!=null&&(this.drawerEl.querySelectorAll(`[data-hop-num="${e}"]`).forEach(e=>e.classList.add(`is-hop-hover`)),t)){let t=this.drawerEl.querySelector(`.hop-row[data-hop-num="${e}"]`);t&&t.scrollIntoView({behavior:`smooth`,block:`nearest`})}}_renderAll(){if(!this.state)return;let e=this._getActivePairData(),t=this._getActiveIncidents(),n=this.replayIndex>=0,r=!!(this.selectedServerId||this.selectedPairKey);this.globe.setData({servers:this.state.servers,pairData:e,carriers:this.state.carriers||{},routeMode:this.routeMode,hasIncident:t.some(e=>e.lasting),isMeasuring:!!(this.state.scheduler&&this.state.scheduler.running)}),this.header.update({state:this.state,routeMode:this.routeMode,hasFocus:r,activeIncidents:t}),this.incidentBanner.update(t,n),this.matrix.update({servers:this.state.servers,pairData:e,sparklines:this.state.sparklines||{},selectedPairKey:this.selectedPairKey,isReplay:n}),this.timeline.update(this.state.timeline||[],this.replayIndex),this._renderCarrierLegend(e),this._renderDrawer()}_renderCarrierLegend(e){if(!(this.routeMode||this.selectedPairKey)||!this.state){this.legendEl.hidden=!0;return}let t=new Set;for(let n of Object.values(e||{}))if(n&&(n.primary_transit_asn&&t.add(n.primary_transit_asn),Array.isArray(n.hops)))for(let e of n.hops)e.primary_asn&&t.add(e.primary_asn);t.size===0&&[1299,3356,174,51167,40021].forEach(e=>t.add(e));let n=this.state.carriers||{},r=[...t].map(e=>j(e,n)).sort((e,t)=>(e.slot??99)-(t.slot??99));this.legendEl.hidden=!1,this.legendEl.innerHTML=`
      <span class="cl-title">CARRIERS</span>
      <div class="cl-items">
        ${r.map(e=>`
          <span class="cl-item">
            <span class="cl-swatch" style="background:${e.color}"></span>
            <span class="cl-name">${N(e.name)}</span>
            <span class="cl-asn mono">AS${e.asn}</span>
          </span>
        `).join(``)}
        <span class="cl-item cl-sep">
          <span class="leg-dot solid"></span><span>Located</span>
        </span>
        <span class="cl-item">
          <span class="leg-dot hollow"></span><span>Interpolated</span>
        </span>
      </div>
    `}_drawerScroll(e){let t=this._drawerKey===e;this._drawerKey=e;let n=[`.drawer-scroll`,`#subway-scroll`],r=t?n.map(e=>{let t=this.drawerEl.querySelector(e);return t?[t.scrollTop,t.scrollLeft]:null}):[],i=t?this.drawerEl.scrollTop:0;return()=>{t&&(this.drawerEl.scrollTop=i,n.forEach((e,t)=>{let n=this.drawerEl.querySelector(e);n&&r[t]&&([n.scrollTop,n.scrollLeft]=r[t])}))}}_renderDrawer(){if(!this.state||!this.selectedServerId&&!this.selectedPairKey){this.drawerEl.classList.remove(`is-open`,`mode-server`,`mode-path`),this.drawerEl.setAttribute(`aria-hidden`,`true`),this._drawerKey=null;return}let e=this._getActivePairData(),t=this.replayIndex>=0,n=this.state.carriers||{};if(this.selectedServerId){let r=this.state.servers.find(e=>e.id===this.selectedServerId);if(!r)return;this.drawerEl.classList.remove(`mode-path`),this.drawerEl.classList.add(`mode-server`,`is-open`),this.drawerEl.setAttribute(`aria-hidden`,`false`);let i=this._drawerScroll(`server:${r.id}`);this.drawerEl.innerHTML=Ae({server:r,servers:this.state.servers,pairData:e,bgp:this.bgpByServer[r.id]||null,bgpLoading:!!this.bgpLoading[r.id],carriers:n,isReplay:t}),i()}else if(this.selectedPairKey){let[r,i]=this.selectedPairKey.split(`->`),a=this.state.servers.find(e=>e.id===r),o=this.state.servers.find(e=>e.id===i);if(!a||!o)return;let s=e[this.selectedPairKey]||null,c=e[`${i}->${r}`]||null;this.drawerEl.classList.remove(`mode-server`),this.drawerEl.classList.add(`mode-path`,`is-open`),this.drawerEl.setAttribute(`aria-hidden`,`false`);let l=this._drawerScroll(`pair:${this.selectedPairKey}`);this.drawerEl.innerHTML=Oe({pairKey:this.selectedPairKey,srcServer:a,dstServer:o,measurement:s,loading:this.selectedPairLoading,carriers:n,routeMode:this.routeMode,isReplay:t,reverseMeasurement:c}),l();let u=this.drawerEl.querySelector(`#subway-scroll`);u&&u.addEventListener(`wheel`,e=>{Math.abs(e.deltaY)>Math.abs(e.deltaX)&&u.scrollWidth>u.clientWidth&&(e.preventDefault(),u.scrollLeft+=e.deltaY)},{passive:!1})}}showToast(e,t=`info`){if(this.simple)return;let n=document.createElement(`div`);n.className=`toast toast-${t}`,n.textContent=e,this.toastRoot.appendChild(n),requestAnimationFrame(()=>n.classList.add(`is-shown`)),setTimeout(()=>{n.classList.remove(`is-shown`),setTimeout(()=>n.remove(),260)},4200)}};if(w().classList.toggle(`is-readonly`,b),w().classList.toggle(`is-embed`,x),new Me,window.__mapStarted=!0,!b)try{let e=/Safari\//.test(navigator.userAgent)&&!/Chrome\//.test(navigator.userAgent)?`Safari`:`another browser`;navigator.sendBeacon(`${v}/client-error`,`ok: in ${e}`)}catch{}