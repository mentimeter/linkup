use rand::distributions::Alphanumeric;
use rand::Rng;

use crate::NameKind;

pub fn new_session_name(
    name_kind: NameKind,
    desired_name: Option<String>,
    exists: &dyn Fn(String) -> bool,
) -> String {
    let mut key = String::new();

    if let Some(name) = desired_name {
        if !exists(name.clone()) {
            key = name;
        }
    }

    if key.is_empty() {
        let mut tried_animal_key = false;
        loop {
            let generated_key = if !tried_animal_key && name_kind == NameKind::Animal {
                tried_animal_key = true;
                generate_unique_animal_key(20, &exists)
            } else {
                random_six_char()
            };

            if !exists(generated_key.clone()) {
                key = generated_key;
                break;
            }
        }
    }

    key
}

fn generate_unique_animal_key(max_attempts: usize, exists: &dyn Fn(String) -> bool) -> String {
    for _ in 0..max_attempts {
        let generated_key = random_animal();
        if !exists(generated_key.clone()) {
            return generated_key;
        }
    }
    // Fallback to SixChar logic
    random_six_char()
}

fn random_animal() -> String {
    let adjective_index = rand::thread_rng().gen_range(0..SHORT_ADJECTIVES.len());
    let animal_index = rand::thread_rng().gen_range(0..ANIMALS.len());

    format!(
        "{}-{}",
        SHORT_ADJECTIVES[adjective_index], ANIMALS[animal_index]
    )
}

fn random_six_char() -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(6)
        .map(char::from)
        .collect()
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
