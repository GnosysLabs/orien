//! Nicknames for wallets, shown by the explorer: "Swift Amber Falcon".
//!
//! Wallets are numbered in the order they first appear on the network (the
//! checkpoint of their first payment or reward, then by address within the
//! same checkpoint), and each number has its own name, so no two wallets
//! ever share one. The numbers are shuffled before they become words, so
//! wallets that appeared together don't look alike. A name, once given, is
//! kept here and never changes.
//!
//! The lists are final: changing or adding a word would change every name.
//! Past 58 million wallets, names gain a fourth word in front.

use crate::store::Store;
use anyhow::Result;
use rusqlite::params;
use std::collections::HashMap;

const ADJECTIVES: [&str; 488] = [
    "Able", "Absolute", "Agile", "Airy", "Alert", "Alive", "Ample", "Ancient", "Angular", "Apt", "Ardent",
    "Artful", "Astral", "Atomic", "August", "Avid", "Awake", "Balmy", "Bold", "Bouncy", "Brave", "Breezy",
    "Brief", "Bright", "Brilliant", "Brisk", "Bubbly", "Buoyant", "Busy", "Calm", "Candid", "Careful",
    "Carefree", "Casual", "Celestial", "Certain", "Charming", "Cheerful", "Chief", "Chilly", "Civic",
    "Classic", "Clean", "Clear", "Clever", "Cloudy", "Coastal", "Cosmic", "Cozy", "Crafty", "Crisp",
    "Curious", "Cushy", "Dapper", "Daring", "Dashing", "Dazzling", "Deep", "Deft", "Dense", "Devoted", "Dewy",
    "Diligent", "Distant", "Dizzy", "Dreamy", "Driven", "Dusky", "Dusty", "Dynamic", "Eager", "Early",
    "Earnest", "Easy", "Eclectic", "Electric", "Elegant", "Elite", "Eminent", "Endless", "Epic", "Equal",
    "Eternal", "Even", "Exact", "Expert", "Fabled", "Fair", "Faithful", "Famous", "Fancy", "Fast", "Fearless",
    "Feisty", "Fervent", "Festive", "Fierce", "Fine", "Firm", "Fit", "Fleet", "Flowing", "Fluffy", "Fluent",
    "Flying", "Focused", "Fond", "Formal", "Fortunate", "Frank", "Free", "Fresh", "Friendly", "Frosty",
    "Frugal", "Full", "Funny", "Fuzzy", "Gallant", "Gentle", "Genuine", "Giant", "Gifted", "Giddy", "Glad",
    "Gleaming", "Glossy", "Glowing", "Graceful", "Gracious", "Grand", "Grateful", "Great", "Groovy",
    "Grounded", "Handy", "Happy", "Hardy", "Harmonic", "Hearty", "Heroic", "Hidden", "Honest", "Hopeful",
    "Humble", "Hushed", "Icy", "Ideal", "Immense", "Infinite", "Inner", "Instant", "Intrepid", "Inventive",
    "Jaunty", "Jolly", "Jovial", "Joyful", "Jubilant", "Just", "Keen", "Kind", "Kindly", "Kinetic", "Knowing",
    "Lasting", "Lavish", "Leafy", "Legendary", "Level", "Light", "Likely", "Limber", "Lively", "Lofty",
    "Logical", "Loyal", "Lucid", "Lucky", "Luminous", "Lunar", "Lush", "Lyrical", "Magic", "Magnetic",
    "Majestic", "Mellow", "Merry", "Mighty", "Mild", "Mindful", "Misty", "Modern", "Modest", "Mossy",
    "Musical", "Mystic", "Natural", "Neat", "Nimble", "Noble", "Nomadic", "Northern", "Novel", "Oaken",
    "Obliging", "Oceanic", "Open", "Optimal", "Orbital", "Organic", "Original", "Outer", "Patient",
    "Peaceful", "Peppy", "Perfect", "Placid", "Playful", "Pleasant", "Plucky", "Plush", "Poetic", "Polar",
    "Polished", "Polite", "Popular", "Precise", "Premium", "Pristine", "Prime", "Proper", "Proud", "Prudent",
    "Pure", "Quaint", "Quick", "Quiet", "Quirky", "Radiant", "Rapid", "Rare", "Ready", "Regal", "Relaxed",
    "Reliable", "Remote", "Resolute", "Restful", "Rich", "Rising", "Robust", "Roaming", "Rocky", "Rolling",
    "Royal", "Rugged", "Rustic", "Sacred", "Safe", "Salty", "Sandy", "Savvy", "Scenic", "Serene", "Sharp",
    "Shining", "Shiny", "Silent", "Silky", "Simple", "Sincere", "Sleek", "Smart", "Smooth", "Snowy", "Snug",
    "Soaring", "Social", "Solar", "Solid", "Sonic", "Southern", "Sparkling", "Speedy", "Spirited", "Splendid",
    "Sporty", "Spry", "Stable", "Starry", "Stately", "Steady", "Stellar", "Stoic", "Stormy", "Strong",
    "Sturdy", "Subtle", "Sunny", "Super", "Supreme", "Sure", "Swift", "Tactful", "Tall", "Tame", "Tender",
    "Thankful", "Thorough", "Thrifty", "Tidal", "Tidy", "Timeless", "Tiny", "Tireless", "Tough", "Tranquil",
    "Tropical", "True", "Trusty", "Tuneful", "Ultra", "Unique", "United", "Upbeat", "Upright", "Urban",
    "Useful", "Valiant", "Vast", "Verdant", "Vibrant", "Vigilant", "Virtual", "Vital", "Vivid", "Vocal",
    "Wandering", "Warm", "Wary", "Wavy", "Wealthy", "Western", "Whimsical", "Whole", "Willing", "Windy",
    "Winged", "Winning", "Wintry", "Wise", "Witty", "Wonderful", "Woody", "Worthy", "Young", "Youthful",
    "Zany", "Zealous", "Zen", "Zesty", "Abundant", "Adept", "Adored", "Advanced", "Adventurous", "Affable",
    "Aglow", "Amiable", "Amused", "Animated", "Arctic", "Aromatic", "Assured", "Attentive", "Audacious",
    "Autumn", "Balanced", "Beaming", "Blazing", "Blissful", "Blithe", "Boundless", "Brainy", "Breathless",
    "Bustling", "Capable", "Captivating", "Cascading", "Centered", "Chipper", "Chromatic", "Clement",
    "Composed", "Confident", "Cordial", "Courageous", "Creative", "Crystal", "Cultured", "Dauntless",
    "Decisive", "Delightful", "Dependable", "Determined", "Dignified", "Discreet", "Drifting", "Durable",
    "Earthy", "Ebullient", "Effortless", "Elated", "Elusive", "Enchanted", "Energetic", "Enduring",
    "Enigmatic", "Ethereal", "Exuberant", "Fiery", "Flawless", "Floating", "Fragrant", "Galactic",
    "Gargantuan", "Glacial", "Glorious", "Harmonious", "Heavenly", "Hypnotic", "Illustrious", "Imaginative",
    "Impish", "Ingenious", "Jazzy", "Kaleidoscopic", "Laughing", "Lilting", "Limitless", "Marvelous",
    "Meandering", "Mirthful", "Moonlit", "Nebular", "Nocturnal", "Opulent", "Peerless", "Perky", "Quantum",
    "Quicksilver", "Rambling", "Resilient", "Resplendent", "Rhythmic", "Roving", "Runic", "Scintillating",
    "Seasoned", "Shimmering", "Skyward", "Slumbering", "Spectral", "Spiral", "Stalwart", "Starlit",
    "Steadfast", "Sublime", "Summery", "Sunlit", "Tenacious", "Thundering", "Timely", "Trailblazing",
    "Twinkling", "Unbound", "Undaunted", "Uplifted", "Valorous", "Venturous", "Vernal", "Virtuous", "Wakeful",
    "Whirling", "Wild", "Wondrous", "Zippy",
];

const COLOURS: [&str; 168] = [
    "Amber", "Amethyst", "Apricot", "Aqua", "Aquamarine", "Ash", "Auburn", "Azure", "Beige", "Berry", "Blue",
    "Blush", "Bronze", "Brass", "Burgundy", "Butter", "Cadet", "Camel", "Canary", "Caramel", "Carmine",
    "Celadon", "Cerise", "Cerulean", "Champagne", "Charcoal", "Cherry", "Chestnut", "Chocolate", "Cinnamon",
    "Citrine", "Citron", "Claret", "Clay", "Cobalt", "Cocoa", "Copper", "Coral", "Cornflower", "Cream",
    "Crimson", "Cyan", "Daffodil", "Denim", "Dove", "Ebony", "Ecru", "Emerald", "Fawn", "Fern", "Flame",
    "Flax", "Fuchsia", "Garnet", "Ginger", "Glacier", "Gold", "Golden", "Granite", "Grape", "Graphite",
    "Gray", "Green", "Harvest", "Hazel", "Heather", "Honey", "Ice", "Indigo", "Iris", "Ivory", "Jade",
    "Jasmine", "Jasper", "Khaki", "Lagoon", "Lapis", "Lavender", "Lemon", "Lilac", "Lime", "Linen", "Magenta",
    "Mahogany", "Maize", "Malachite", "Mango", "Maple", "Marigold", "Maroon", "Mauve", "Melon", "Mint",
    "Mocha", "Moss", "Mulberry", "Mustard", "Navy", "Nectar", "Ochre", "Olive", "Onyx", "Opal", "Orange",
    "Orchid", "Oyster", "Peach", "Pear", "Pearl", "Periwinkle", "Pewter", "Pine", "Pink", "Pistachio",
    "Platinum", "Plum", "Poppy", "Porcelain", "Powder", "Pumpkin", "Purple", "Quartz", "Raspberry", "Rose",
    "Ruby", "Russet", "Rust", "Saffron", "Sage", "Salmon", "Sand", "Sapphire", "Scarlet", "Seafoam", "Sepia",
    "Shadow", "Sienna", "Silver", "Sky", "Slate", "Smoke", "Snow", "Steel", "Stone", "Storm", "Straw",
    "Sunset", "Tan", "Tangerine", "Taupe", "Teal", "Terracotta", "Thistle", "Topaz", "Tulip", "Turquoise",
    "Ultramarine", "Umber", "Vanilla", "Velvet", "Verdigris", "Vermilion", "Violet", "Walnut", "Wheat",
    "Wine", "Wisteria", "Zinc",
];

const NOUNS: [&str; 712] = [
    "Aardvark", "Albatross", "Alder", "Alpaca", "Anchor", "Anemone", "Antelope", "Antler", "Arch", "Arrow",
    "Aspen", "Asteroid", "Atlas", "Atoll", "Aurora", "Avalanche", "Avocet", "Axolotl", "Badger", "Bamboo",
    "Banyan", "Barracuda", "Basin", "Bay", "Bayou", "Beacon", "Bear", "Beetle", "Beluga", "Birch", "Bison",
    "Bittern", "Blossom", "Bluebell", "Bluff", "Boar", "Bobcat", "Bolt", "Boulder", "Bramble", "Breeze",
    "Brook", "Buffalo", "Bunting", "Butte", "Butterfly", "Cactus", "Caiman", "Canyon", "Capybara", "Caracal",
    "Cardinal", "Caribou", "Cascade", "Cassowary", "Catfish", "Cavern", "Cedar", "Chamois", "Chameleon",
    "Cheetah", "Chinchilla", "Cicada", "Cinder", "Cirrus", "Clam", "Cliff", "Cloud", "Clover", "Cobra",
    "Comet", "Compass", "Condor", "Conifer", "Cormorant", "Cosmos", "Cougar", "Cove", "Coyote", "Crab",
    "Crane", "Crater", "Creek", "Cricket", "Crocus", "Crow", "Current", "Cyclone", "Cypress", "Dahlia",
    "Daisy", "Dawn", "Delta", "Desert", "Dingo", "Dipper", "Dolphin", "Dragon", "Dragonfly", "Drift", "Dune",
    "Dusk", "Eagle", "Echo", "Eclipse", "Eel", "Egret", "Eland", "Elk", "Elm", "Ember", "Emu", "Equinox",
    "Ermine", "Estuary", "Falcon", "Fennec", "Ferret", "Finch", "Fir", "Firefly", "Fjord", "Flamingo",
    "Flint", "Forest", "Fox", "Frog", "Frost", "Gale", "Galaxy", "Gazelle", "Gecko", "Geyser", "Glade",
    "Glen", "Gnu", "Goldfinch", "Goose", "Gopher", "Gorge", "Grasshopper", "Grebe", "Grizzly", "Grouse",
    "Grove", "Gull", "Halo", "Hare", "Harrier", "Hawk", "Heath", "Hedgehog", "Heron", "Hickory", "Hippo",
    "Hollow", "Hornet", "Horizon", "Hummingbird", "Husky", "Hyacinth", "Ibex", "Ibis", "Iguana", "Impala",
    "Island", "Isle", "Ivy", "Jackal", "Jaguar", "Jay", "Jellyfish", "Juniper", "Kangaroo", "Kestrel",
    "Kingfisher", "Kite", "Kiwi", "Koala", "Koi", "Krill", "Lark", "Lantern", "Larch", "Leopard", "Lichen",
    "Lighthouse", "Lily", "Lion", "Lizard", "Llama", "Lobster", "Lotus", "Lynx", "Macaw", "Magpie", "Mallard",
    "Manatee", "Mantis", "Marmot", "Marten", "Meadow", "Meerkat", "Merlin", "Mesa", "Meteor", "Mink",
    "Minnow", "Mist", "Mole", "Mongoose", "Moon", "Moose", "Moth", "Mountain", "Mustang", "Narwhal", "Nebula",
    "Newt", "Nightingale", "Nova", "Oak", "Ocelot", "Octopus", "Okapi", "Orbit", "Orca", "Oriole", "Osprey",
    "Ostrich", "Otter", "Owl", "Panda", "Panther", "Parrot", "Peacock", "Pebble", "Pelican", "Penguin",
    "Peregrine", "Petrel", "Pheasant", "Pike", "Plover", "Polaris", "Pony", "Porcupine", "Prairie", "Pulsar",
    "Puffin", "Puma", "Python", "Quail", "Quasar", "Quokka", "Rabbit", "Raccoon", "Rain", "Rainbow", "Raven",
    "Ray", "Redwood", "Reef", "Reindeer", "Ridge", "River", "Robin", "Rook", "Rover", "Salamander",
    "Sandpiper", "Sapling", "Satellite", "Sequoia", "Serval", "Shark", "Shell", "Shore", "Shrike", "Shrimp",
    "Sierra", "Skylark", "Sloth", "Snail", "Snipe", "Sparrow", "Spruce", "Squid", "Squirrel", "Star",
    "Starling", "Stingray", "Stoat", "Stork", "Stream", "Sturgeon", "Summit", "Sun", "Swan", "Sycamore",
    "Tapir", "Tern", "Thrush", "Thunder", "Tide", "Tiger", "Timber", "Toucan", "Tortoise", "Trout", "Tundra",
    "Turtle", "Unicorn", "Urchin", "Valley", "Vapor", "Viper", "Vole", "Volcano", "Wagtail", "Walrus",
    "Warbler", "Waterfall", "Wave", "Whale", "Willow", "Wolf", "Wolverine", "Wombat", "Woodpecker", "Wren",
    "Yak", "Yarrow", "Yew", "Zebra", "Zenith", "Zephyr", "Acorn", "Agate", "Alcove", "Almond", "Ammonite",
    "Anvil", "Apple", "Aquifer", "Arbor", "Archipelago", "Armadillo", "Astrolabe", "Azalea", "Balloon",
    "Banner", "Barley", "Basil", "Bell", "Blizzard", "Bluejay", "Bonfire", "Bonsai", "Bouquet", "Bridge",
    "Bubble", "Buckeye", "Bud", "Bulb", "Cabin", "Cairn", "Camellia", "Campfire", "Candle", "Canoe", "Canopy",
    "Cape", "Carnation", "Carousel", "Castle", "Catkin", "Cello", "Chalice", "Channel", "Chariot", "Chime",
    "Chord", "Citadel", "Clarinet", "Coconut", "Cocoon", "Constellation", "Cornet", "Cotton", "Crescent",
    "Crest", "Cumulus", "Cupola", "Daybreak", "Dewdrop", "Diamond", "Dogwood", "Dome", "Drizzle", "Drum",
    "Easel", "Edelweiss", "Evergreen", "Feather", "Ferry", "Fiddle", "Fig", "Flare", "Flute", "Foam",
    "Fountain", "Foxglove", "Frigate", "Gable", "Galleon", "Garden", "Gem", "Glider", "Globe", "Glow",
    "Gondola", "Gourd", "Grapevine", "Gust", "Hailstone", "Hammock", "Harbor", "Harp", "Helix", "Hemlock",
    "Hibiscus", "Highland", "Hill", "Holly", "Honeybee", "Horn", "Iceberg", "Icicle", "Inlet", "Jetty",
    "Jewel", "Kayak", "Kelp", "Kettle", "Keystone", "Kiln", "Knoll", "Lake", "Laurel", "Linden", "Lodestar",
    "Lute", "Lyre", "Magnolia", "Mandolin", "Mangrove", "Marble", "Marsh", "Mast", "Meridian", "Mirage",
    "Monsoon", "Mosaic", "Myrtle", "Nettle", "Nimbus", "Nutmeg", "Oasis", "Obsidian", "Ocean", "Orchard",
    "Origami", "Paddle", "Pagoda", "Palm", "Papaya", "Parasol", "Pathway", "Pavilion", "Peak", "Peony",
    "Pepper", "Pinecone", "Pinnacle", "Plateau", "Plume", "Pond", "Prism", "Quill", "Quiver", "Radish",
    "Raft", "Rapids", "Reed", "Ribbon", "Ripple", "Rocket", "Rosebud", "Rowan", "Rudder", "Rune", "Saddle",
    "Sail", "Sailboat", "Sandstone", "Scroll", "Seashell", "Sextant", "Shamrock", "Shoal", "Skiff", "Sleet",
    "Snowdrop", "Snowflake", "Solstice", "Sonnet", "Spark", "Sparrowhawk", "Spire", "Sprout", "Squall",
    "Stardust", "Steppe", "Strand", "Sunbeam", "Sundial", "Sunflower", "Sunrise", "Tamarind", "Teacup",
    "Telescope", "Tempest", "Thyme", "Torch", "Tower", "Trail", "Trellis", "Trumpet", "Tumbleweed", "Turnip",
    "Twilight", "Typhoon", "Umbrella", "Upland", "Vale", "Vessel", "Vine", "Violin", "Voyager", "Watermelon",
    "Whirlwind", "Wildflower", "Windmill", "Wisp", "Xylophone", "Zinnia", "Albacore", "Anchovy", "Angelfish",
    "Bandicoot", "Barnacle", "Bass", "Bluebird", "Bluegill", "Bullfinch", "Chickadee", "Chipmunk", "Cockatoo",
    "Coelacanth", "Crayfish", "Curlew", "Cuttlefish", "Damselfly", "Dormouse", "Dragonet", "Dugong", "Dunlin",
    "Eider", "Falconet", "Flounder", "Fulmar", "Gannet", "Gerbil", "Goby", "Godwit", "Grayling", "Guillemot",
    "Haddock", "Halibut", "Hamster", "Hoopoe", "Jackdaw", "Jerboa", "Kinkajou", "Kittiwake", "Lapwing",
    "Linnet", "Loon", "Lorikeet", "Mackerel", "Marlin", "Merganser", "Mockingbird", "Moorhen", "Nautilus",
    "Nuthatch", "Opossum", "Oystercatcher", "Pangolin", "Partridge", "Perch", "Pipit", "Platypus", "Pollock",
    "Ptarmigan", "Quetzal", "Redstart", "Roadrunner", "Sailfish", "Sardine", "Scallop", "Seahorse", "Seal",
    "Skink", "Skipper", "Sunbird", "Swordfish", "Tadpole", "Tarpon", "Toad", "Treefrog", "Triggerfish",
    "Tuna", "Turbot", "Vireo", "Wallaby", "Waxwing", "Whimbrel", "Whippet", "Wildcat", "Woodcock", "Zander",
    "Andromeda", "Antares", "Apogee", "Aquila", "Arcturus", "Betelgeuse", "Callisto", "Capella", "Cassiopeia",
    "Centauri", "Cepheus", "Ceres", "Cygnus", "Deneb", "Draco", "Europa", "Ganymede", "Hydra", "Io",
    "Jupiter", "Lyra", "Mars", "Mercury", "Neptune", "Orion", "Pegasus", "Perseus", "Phoenix", "Pleiades",
    "Procyon", "Pluto", "Regulus", "Rigel", "Saturn", "Sirius", "Spica", "Titan", "Triton", "Vega", "Venus",
    "Bloom", "Dewfall", "Driftwood", "Eventide", "Fireweed", "Flicker", "Glimmer", "Gloaming", "Harmony",
    "Haven", "Hearth", "Lumen", "Moonbeam", "Moonstone", "Morning", "Nightfall", "Northstar", "Rainfall",
    "Shimmer", "Skyline", "Snowfall", "Starfall", "Starlight", "Sunburst", "Sunstone", "Tidewater",
    "Waypoint", "Wildwood", "Windward",
];

/// Three-word names there are.
const NAMES: u64 = (ADJECTIVES.len() * COLOURS.len() * NOUNS.len()) as u64;
/// Shuffles the numbers: shares no factor with `NAMES`, so every number
/// below it lands on a different one.
const SHUFFLE: u128 = 2_654_435_761;
const SHIFT: u128 = 40_503_119;

/// The name of wallet `number` (from 0).
pub fn nickname(number: u64) -> String {
    let at = ((number % NAMES) as u128 * SHUFFLE + SHIFT) % NAMES as u128;
    let at = at as usize;
    let noun = NOUNS[at % NOUNS.len()];
    let colour = COLOURS[(at / NOUNS.len()) % COLOURS.len()];
    let adjective = ADJECTIVES[at / (NOUNS.len() * COLOURS.len())];
    let name = format!("{adjective} {colour} {noun}");
    match number / NAMES {
        0 => name,
        round => format!("{} {name}", ADJECTIVES[(round as usize - 1) % ADJECTIVES.len()]),
    }
}

impl Store {
    pub(crate) fn create_nickname_table(&self) -> Result<()> {
        self.db.execute_batch(
            "CREATE TABLE IF NOT EXISTS nicknames (
                 address TEXT PRIMARY KEY,
                 number INTEGER NOT NULL UNIQUE,
                 first_epoch INTEGER NOT NULL
             );",
        )?;
        Ok(())
    }

    /// Numbers the wallets among `seen` (address, checkpoint it first
    /// appeared in) that have none yet, in order of first appearance, then
    /// address. Returns how many were named.
    pub fn name_wallets(&mut self, seen: &[(String, u64)]) -> Result<usize> {
        let tx = self.db.transaction()?;
        let mut new: Vec<(u64, String)> = Vec::new();
        {
            let mut known = tx.prepare("SELECT 1 FROM nicknames WHERE address = ?1")?;
            for (address, epoch) in seen {
                if !known.exists([address])? {
                    new.push((*epoch, address.clone()));
                }
            }
        }
        new.sort();
        new.dedup_by(|a, b| a.1 == b.1);
        let next: i64 = tx.query_row("SELECT COALESCE(MAX(number) + 1, 0) FROM nicknames", [], |row| row.get(0))?;
        for (i, (epoch, address)) in new.iter().enumerate() {
            tx.execute(
                "INSERT INTO nicknames (address, number, first_epoch) VALUES (?1, ?2, ?3)",
                params![address, next + i as i64, *epoch as i64],
            )?;
        }
        tx.commit()?;
        Ok(new.len())
    }

    /// The nicknames of those of `addresses` that have one.
    pub fn nicknames(&self, addresses: &[String]) -> Result<HashMap<String, String>> {
        let mut found = HashMap::new();
        let mut stmt = self.db.prepare("SELECT number FROM nicknames WHERE address = ?1")?;
        for address in addresses {
            let address = address.to_lowercase();
            if let Some(number) = stmt.query_row([&address], |row| row.get::<_, i64>(0)).ok() {
                found.insert(address, nickname(number as u64));
            }
        }
        Ok(found)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn every_number_gets_its_own_name() {
        assert_eq!(num_integer_gcd(NAMES as u128, SHUFFLE), 1);
        let names: HashSet<String> = (0..200_000).map(nickname).collect();
        assert_eq!(names.len(), 200_000);
        // Past the three-word names, a fourth word in front.
        assert_ne!(nickname(NAMES), nickname(0));
        assert!(nickname(NAMES).ends_with(&nickname(0)));
    }

    #[test]
    fn no_word_is_in_two_lists() {
        let all: Vec<&str> = ADJECTIVES.iter().chain(COLOURS.iter()).chain(NOUNS.iter()).copied().collect();
        let unique: HashSet<&str> = all.iter().copied().collect();
        assert_eq!(all.len(), unique.len());
    }

    #[test]
    fn names_are_given_in_order_of_first_appearance_and_kept() {
        let mut store = Store::in_memory().unwrap();
        let a = "aa".repeat(32);
        let b = "bb".repeat(32);
        let c = "cc".repeat(32);
        assert_eq!(store.name_wallets(&[(b.clone(), 5), (a.clone(), 9)]).unwrap(), 2);
        // Named again, or seen later: unchanged.
        assert_eq!(store.name_wallets(&[(a.clone(), 1), (c.clone(), 3)]).unwrap(), 1);
        let names = store.nicknames(&[a.clone(), b.clone(), c.clone(), "dd".repeat(32)]).unwrap();
        assert_eq!(names[&b], nickname(0));
        assert_eq!(names[&a], nickname(1));
        assert_eq!(names[&c], nickname(2));
        assert_eq!(names.len(), 3);
    }

    fn num_integer_gcd(mut a: u128, mut b: u128) -> u128 {
        while b != 0 {
            (a, b) = (b, a % b);
        }
        a
    }
}
