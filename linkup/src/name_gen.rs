use rand::{
    distr::{Alphanumeric, SampleString},
    seq::IndexedRandom,
};
use sha2::{Digest, Sha256};

pub fn random_animal() -> String {
    let mut rand = rand::rng();

    let adjective = SHORT_ADJECTIVES
        .choose(&mut rand)
        .expect("Adjectives slice should not be empty");
    let animal = ANIMALS
        .choose(&mut rand)
        .expect("Animals slice should not be empty");

    format!("{adjective}-{animal}")
}

pub fn deterministic_six_char_hash(input: &str) -> String {
    let mut hasher = Sha256::new();

    hasher.update(input);

    let result = hasher.finalize();
    let hex_string = hex::encode(result);

    // Truncate the hexadecimal string to 6 characters
    hex_string[..6].to_string()
}

pub fn random_six_char() -> String {
    Alphanumeric
        .sample_string(&mut rand::rng(), 6)
        .to_lowercase()
}

const ANIMALS: [&str; 37] = [
    "ant", "bat", "bison", "camel", "cat", "cow", "crab", "deer", "dog", "duck", "eagle", "fish",
    "fox", "frog", "gecko", "goat", "goose", "hare", "horse", "koala", "lion", "lynx", "mole",
    "mouse", "otter", "panda", "pig", "prawn", "puma", "quail", "sheep", "sloth", "snake", "swan",
    "tiger", "wolf", "zebra",
];

const SHORT_ADJECTIVES: [&str; 127] = [
    "able", "acid", "adept", "aged", "airy", "ajar", "awry", "back", "bare", "beefy", "big",
    "blond", "blue", "bold", "bossy", "brave", "brief", "broad", "busy", "calm", "cheap", "chill",
    "clean", "coy", "crazy", "curvy", "cute", "damp", "dear", "deep", "dizzy", "dopey", "drunk",
    "dry", "dull", "dusty", "easy", "edgy", "fiery", "fancy", "fat", "few", "fine", "flat", "foxy",
    "fresh", "frisky", "full", "fun", "glad", "grand", "great", "green", "happy", "hard", "hazy",
    "icy", "jolly", "jumpy", "kind", "lame", "late", "leafy", "light", "loyal", "lucky", "mad",
    "mean", "neat", "new", "nice", "noble", "odd", "old", "perky", "proud", "quick", "quiet",
    "rare", "red", "ripe", "rotten", "safe", "salty", "sandy", "scary", "shaky", "sharp", "short",
    "shy", "silly", "sleek", "slim", "slow", "small", "smart", "smug", "snappy", "soggy", "sour",
    "spicy", "stale", "stark", "steep", "sticky", "stout", "super", "sweet", "sunny", "tall",
    "tame", "tart", "tasty", "tepid", "tiny", "tipsy", "tough", "true", "vague", "vivid", "warm",
    "weak", "wild", "wise", "wooden", "witty", "zesty",
];
