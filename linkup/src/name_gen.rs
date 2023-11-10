use rand::distributions::Alphanumeric;
use rand::Rng;

pub fn random_animal() -> String {
    let adjective_index = rand::thread_rng().gen_range(0..SHORT_ADJECTIVES.len());
    let animal_index = rand::thread_rng().gen_range(0..ANIMALS.len());

    format!(
        "{}-{}",
        SHORT_ADJECTIVES[adjective_index], ANIMALS[animal_index]
    )
}

pub fn random_six_char() -> String {
    let string: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(6)
        .map(char::from)
        .collect();

    string.to_lowercase()
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
