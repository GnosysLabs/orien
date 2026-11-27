//! Nicknames for wallets, shown by the explorer: an adjective, a colour and
//! an animal, as "Swift Amber Falcon".
//!
//! Wallets are numbered in the order they first appear on the network (the
//! checkpoint of their first payment or reward, then by address within the
//! same checkpoint), and each number has its own name, so no two wallets
//! ever share one. The numbers are shuffled before they become words, so
//! wallets that appeared together don't look alike. A name, once given, is
//! kept here and never changes.
//!
//! The lists are final: changing or adding a word would change every name.
//! Past 11 million wallets, names gain a fourth word in front.

use crate::store::Store;
use anyhow::Result;
use rusqlite::params;
use std::collections::HashMap;

const ADJECTIVES: [&str; 322] = [
    "Agile", "Airy", "Alert", "Ancient", "Ardent", "Artful", "Astral", "Autumn", "Bold", "Bouncy", "Brave",
    "Breezy", "Bright", "Brisk", "Bubbly", "Buoyant", "Calm", "Candid", "Carefree", "Celestial", "Charming",
    "Cheerful", "Chilly", "Clever", "Cloudy", "Coastal", "Cosmic", "Cozy", "Crafty", "Crisp", "Curious",
    "Dapper", "Daring", "Dashing", "Dazzling", "Deep", "Deft", "Dreamy", "Drifting", "Dusky", "Dusty",
    "Eager", "Early", "Electric", "Elegant", "Endless", "Epic", "Eternal", "Fabled", "Fair", "Faithful",
    "Fearless", "Feisty", "Fierce", "Fleet", "Flying", "Fond", "Free", "Fresh", "Friendly", "Frosty", "Fuzzy",
    "Gallant", "Gentle", "Giant", "Gifted", "Glad", "Gleaming", "Glossy", "Glowing", "Graceful", "Grand",
    "Happy", "Hardy", "Hearty", "Heroic", "Hidden", "Honest", "Hopeful", "Humble", "Hushed", "Icy",
    "Intrepid", "Jaunty", "Jolly", "Jovial", "Joyful", "Keen", "Kind", "Kindly", "Lively", "Lofty", "Loyal",
    "Lucky", "Luminous", "Lunar", "Lush", "Magic", "Majestic", "Mellow", "Merry", "Mighty", "Mild", "Misty",
    "Moonlit", "Mossy", "Mystic", "Nimble", "Noble", "Nomadic", "Northern", "Plucky", "Playful", "Polar",
    "Proud", "Quick", "Quiet", "Quirky", "Radiant", "Rapid", "Rare", "Regal", "Rising", "Roaming", "Rocky",
    "Royal", "Rugged", "Rustic", "Sandy", "Serene", "Shining", "Shiny", "Silent", "Silky", "Sleek", "Sleepy",
    "Smooth", "Snowy", "Snug", "Soaring", "Solar", "Sparkling", "Speedy", "Spirited", "Splendid", "Spry",
    "Starry", "Stately", "Steady", "Stellar", "Stormy", "Strong", "Sturdy", "Sunny", "Swift", "Tall", "Tidal",
    "Tiny", "Tireless", "Tranquil", "Tropical", "True", "Trusty", "Valiant", "Vast", "Velvet", "Vivid",
    "Wandering", "Warm", "Wary", "Wild", "Windy", "Winged", "Wintry", "Wise", "Witty", "Wondrous", "Zesty",
    "Zippy", "Arctic", "Blazing", "Blissful", "Boundless", "Dauntless", "Distant", "Drowsy", "Dynamic",
    "Elusive", "Enchanted", "Ethereal", "Fiery", "Floating", "Galactic", "Gilded", "Glacial", "Glorious",
    "Hazy", "Heavenly", "Humming", "Laughing", "Leaping", "Lilting", "Midnight", "Mirthful", "Nebular",
    "Nocturnal", "Prancing", "Prowling", "Quantum", "Racing", "Rambling", "Roving", "Ruffled", "Scampering",
    "Shimmering", "Singing", "Skyward", "Slumbering", "Sprightly", "Starlit", "Stealthy", "Sunlit",
    "Thundering", "Twinkling", "Untamed", "Velvety", "Wakeful", "Whirling", "Whispering", "Zealous", "Plush",
    "Peppy", "Perky", "Placid", "Patient", "Peaceful", "Pure", "Balmy", "Blithe", "Canny", "Chipper",
    "Cuddly", "Dainty", "Dandy", "Downy", "Fluffy", "Frisky", "Gleeful", "Jazzy", "Nifty", "Quaint",
    "Scrappy", "Snappy", "Spunky", "Toasty", "Woolly", "Zany", "Adventurous", "Affable", "Amiable", "Bashful",
    "Beaming", "Bountiful", "Brainy", "Bustling", "Capable", "Clement", "Cordial", "Courageous", "Dutiful",
    "Earnest", "Easygoing", "Feathered", "Festive", "Flashy", "Fleecy", "Gutsy", "Harmonic", "Jubilant",
    "Lucid", "Mindful", "Modest", "Musical", "Nautical", "Oceanic", "Orbital", "Poetic", "Prized", "Rhythmic",
    "Robust", "Rolling", "Sacred", "Savvy", "Scenic", "Seafaring", "Shy", "Sincere", "Skillful", "Spotted",
    "Stalwart", "Steadfast", "Striped", "Sublime", "Summery", "Sunkissed", "Tender", "Thrifty", "Timeless",
    "Trailblazing", "Upbeat", "Vernal", "Vigilant", "Vibrant", "Virtuous", "Waving", "Wholesome", "Whimsical",
    "Wily", "Wispy", "Wonderful", "Worthy", "Youthful",
];

const COLOURS: [&str; 78] = [
    "Amber", "Aqua", "Auburn", "Azure", "Beige", "Blue", "Blush", "Bronze", "Cerulean", "Charcoal", "Cobalt",
    "Copper", "Coral", "Cream", "Crimson", "Cyan", "Emerald", "Fuchsia", "Garnet", "Gold", "Gray", "Green",
    "Hazel", "Indigo", "Ivory", "Jade", "Khaki", "Lavender", "Lilac", "Lime", "Magenta", "Mahogany", "Maroon",
    "Mauve", "Mint", "Navy", "Ochre", "Olive", "Opal", "Orange", "Pearl", "Periwinkle", "Pewter", "Pink",
    "Platinum", "Plum", "Purple", "Red", "Rose", "Ruby", "Russet", "Rust", "Saffron", "Sage", "Sapphire",
    "Scarlet", "Sepia", "Sienna", "Silver", "Slate", "Steel", "Tan", "Taupe", "Teal", "Topaz", "Turquoise",
    "Umber", "Vermilion", "Violet", "Cinnamon", "Citrine", "Claret", "Ginger", "Honey", "Lemon", "Marigold",
    "Mustard", "Sand",
];

const NOUNS: [&str; 459] = [
    "Aardvark", "Aardwolf", "Addax", "Alpaca", "Anteater", "Antelope", "Armadillo", "Badger", "Bandicoot",
    "Bear", "Binturong", "Bison", "Boar", "Bobcat", "Bongo", "Buffalo", "Camel", "Capybara", "Caracal",
    "Caribou", "Cheetah", "Chinchilla", "Chipmunk", "Civet", "Coati", "Cougar", "Coyote", "Deer", "Dhole",
    "Dingo", "Dolphin", "Dormouse", "Dugong", "Eland", "Elephant", "Elk", "Ermine", "Fennec", "Ferret", "Fox",
    "Gazelle", "Genet", "Gerbil", "Giraffe", "Gnu", "Gopher", "Grizzly", "Hamster", "Hare", "Hedgehog",
    "Hippo", "Ibex", "Impala", "Jackal", "Jaguar", "Jerboa", "Kangaroo", "Kinkajou", "Koala", "Kudu",
    "Leopard", "Lion", "Llama", "Lynx", "Manatee", "Mammoth", "Margay", "Marmot", "Marten", "Meerkat", "Mink",
    "Mole", "Mongoose", "Moose", "Mouflon", "Muskox", "Narwhal", "Numbat", "Ocelot", "Okapi", "Opossum",
    "Oryx", "Otter", "Panda", "Pangolin", "Panther", "Platypus", "Pika", "Porcupine", "Porpoise", "Possum",
    "Pronghorn", "Puma", "Quokka", "Quoll", "Rabbit", "Raccoon", "Reindeer", "Rhino", "Sable", "Seal",
    "Serval", "Sloth", "Springbok", "Squirrel", "Stoat", "Tapir", "Tiger", "Vicuna", "Vole", "Wallaby",
    "Walrus", "Warthog", "Whale", "Wildcat", "Wolf", "Wolverine", "Wombat", "Yak", "Zebra", "Zebu", "Beluga",
    "Orca", "Muntjac", "Gaur", "Takin", "Saiga", "Chamois", "Markhor", "Nilgai", "Lemming", "Hyrax", "Tenrec",
    "Echidna", "Wallaroo", "Bilby", "Fossa", "Mustang", "Pony", "Foal", "Stallion", "Husky", "Beagle",
    "Corgi", "Collie", "Terrier", "Poodle", "Whippet", "Kitten", "Puppy", "Lamb", "Bunny", "Albatross", "Auk",
    "Avocet", "Barbet", "Bittern", "Bluebird", "Bluejay", "Bobolink", "Bowerbird", "Bullfinch", "Bunting",
    "Canary", "Cardinal", "Cassowary", "Chaffinch", "Chickadee", "Chiffchaff", "Condor", "Coot", "Cormorant",
    "Corncrake", "Crane", "Crossbill", "Crow", "Cuckoo", "Curlew", "Dipper", "Dotterel", "Dove", "Dunlin",
    "Eagle", "Egret", "Eider", "Emu", "Falcon", "Finch", "Flamingo", "Flycatcher", "Fulmar", "Gannet",
    "Godwit", "Goldcrest", "Goldfinch", "Goose", "Goshawk", "Grackle", "Grebe", "Grosbeak", "Grouse",
    "Guillemot", "Gull", "Harrier", "Hawk", "Heron", "Hoopoe", "Hornbill", "Hummingbird", "Ibis", "Jacana",
    "Jackdaw", "Jay", "Junco", "Kakapo", "Kea", "Kestrel", "Killdeer", "Kingbird", "Kingfisher", "Kite",
    "Kittiwake", "Kiwi", "Kookaburra", "Lapwing", "Lark", "Linnet", "Loon", "Lorikeet", "Lovebird",
    "Lyrebird", "Macaw", "Magpie", "Mallard", "Martin", "Meadowlark", "Merganser", "Merlin", "Mockingbird",
    "Moorhen", "Murrelet", "Nighthawk", "Nightingale", "Nightjar", "Nuthatch", "Oriole", "Osprey", "Ostrich",
    "Ouzel", "Owl", "Oystercatcher", "Parakeet", "Parrot", "Partridge", "Peacock", "Pelican", "Penguin",
    "Peregrine", "Petrel", "Pheasant", "Phoebe", "Pigeon", "Pintail", "Pipit", "Plover", "Puffin", "Quail",
    "Quetzal", "Raven", "Redpoll", "Redstart", "Rhea", "Roadrunner", "Robin", "Rook", "Rosella", "Sanderling",
    "Sandpiper", "Shearwater", "Shoebill", "Shrike", "Siskin", "Skua", "Skylark", "Snipe", "Sparrow",
    "Spoonbill", "Starling", "Stilt", "Stonechat", "Stork", "Sunbird", "Swan", "Tanager", "Tern", "Thrasher",
    "Thrush", "Toucan", "Towhee", "Treecreeper", "Turaco", "Turnstone", "Vireo", "Wagtail", "Warbler",
    "Waxwing", "Weaver", "Wheatear", "Whimbrel", "Wigeon", "Woodcock", "Woodpecker", "Wren", "Hoatzin",
    "Bellbird", "Fantail", "Firecrest", "Honeyeater", "Kinglet", "Motmot", "Pitta", "Trogon", "Tui",
    "Wryneck", "Albacore", "Anchovy", "Angelfish", "Barracuda", "Bass", "Bluefin", "Bonito", "Carp",
    "Catfish", "Clownfish", "Cod", "Coelacanth", "Eel", "Flounder", "Goby", "Grouper", "Guppy", "Haddock",
    "Halibut", "Herring", "Koi", "Mackerel", "Mahimahi", "Manta", "Marlin", "Minnow", "Moray", "Mullet",
    "Perch", "Pike", "Piranha", "Pollock", "Pufferfish", "Salmon", "Sardine", "Seahorse", "Shark", "Skate",
    "Snapper", "Stingray", "Sturgeon", "Sunfish", "Swordfish", "Tarpon", "Tetra", "Trout", "Tuna", "Turbot",
    "Wahoo", "Walleye", "Zander", "Lionfish", "Triggerfish", "Parrotfish", "Sailfish", "Bluegill", "Grayling",
    "Remora", "Octopus", "Squid", "Cuttlefish", "Nautilus", "Jellyfish", "Starfish", "Urchin", "Crab",
    "Lobster", "Shrimp", "Prawn", "Krill", "Clam", "Oyster", "Scallop", "Mussel", "Barnacle", "Anemone",
    "Axolotl", "Adder", "Anole", "Boa", "Caiman", "Chameleon", "Cobra", "Crocodile", "Gecko", "Gharial",
    "Iguana", "Lizard", "Mamba", "Newt", "Python", "Salamander", "Skink", "Tortoise", "Turtle", "Viper",
    "Frog", "Toad", "Bullfrog", "Treefrog", "Tuatara", "Terrapin", "Bee", "Beetle", "Butterfly", "Cicada",
    "Cricket", "Dragonfly", "Firefly", "Grasshopper", "Ladybug", "Mantis", "Moth", "Damselfly", "Bumblebee",
    "Honeybee", "Katydid", "Scarab", "Snail", "Glowworm", "Dragon", "Griffin", "Phoenix", "Unicorn",
    "Pegasus", "Kraken", "Hydra", "Wyvern", "Sphinx", "Basilisk", "Roc", "Leviathan", "Yeti", "Selkie",
    "Chimera",
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
