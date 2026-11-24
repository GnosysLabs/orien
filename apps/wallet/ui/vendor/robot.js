// A robot for each Orien Network account with no picture of its own, drawn
// here on the device by DiceBear (MIT, @dicebear/core 10.7.0) in the
// "Bottts Neutral" style (Bottts by Pablo Stanley), with DiceBear's
// "Full Cast" preset: every robot gets the surface texture. Never fetched
// from anywhere: no outside service learns who looks at whom. The website
// draws the same robot from the same seed (the username, trimmed and in
// lower case) and options, so an account looks the same everywhere.
import { Avatar, Style } from "./dicebear/index.js";
import botttsNeutral from "./bottts-neutral.js";

const style = new Style(botttsNeutral);
const drawn = new Map();
window.orienRobot = (seed) => {
  if (!drawn.has(seed)) {
    if (drawn.size > 500) drawn.delete(drawn.keys().next().value);
    drawn.set(seed, "data:image/svg+xml;utf8," + encodeURIComponent(new Avatar(style, { seed, textureProbability: 100 }).toString()));
  }
  return drawn.get(seed);
};
window.dispatchEvent(new Event("orien-robots"));
