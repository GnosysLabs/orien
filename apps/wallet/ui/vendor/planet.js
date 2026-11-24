// A planet for each Orien Network account with no picture of its own, drawn
// here on the device by DiceBear (MIT, @dicebear/core 10.7.0) from the
// "Planets" style (CC0). Never fetched from anywhere: no outside service
// learns who looks at whom. The website draws the same planet from the
// same seed, so an account looks the same everywhere.
import { Avatar, Style } from "./dicebear/index.js";
import planets from "./planets.js";

const style = new Style(planets);
const drawn = new Map();
window.orienPlanet = (id) => {
  const seed = `orien:planets:v1:${id}`;
  if (!drawn.has(seed)) {
    if (drawn.size > 500) drawn.delete(drawn.keys().next().value);
    drawn.set(seed, "data:image/svg+xml;utf8," + encodeURIComponent(new Avatar(style, { seed }).toString()));
  }
  return drawn.get(seed);
};
window.dispatchEvent(new Event("orien-planets"));
