use rand::seq::IndexedRandom;

const WORDLIST: &str = include_str!("wordlist.txt");
const WORD_COUNT: usize = 6;

fn generate(words: &[&str], n: usize, rng: &mut impl rand::Rng) -> String {
    (0..n)
        .map(|_| *words.choose(rng).expect("wordlist is non-empty"))
        .collect::<Vec<_>>()
        .join("-")
}

fn main() {
    let words: Vec<&str> = WORDLIST.lines().collect();
    let mut rng = rand::rng();
    println!("{}", generate(&words, WORD_COUNT, &mut rng));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn produces_six_words_of_at_least_five_letters() {
        let words = ["correct", "battery", "staple", "pasture", "rebuild"];
        let mut rng = rand::rng();
        let password = generate(&words, WORD_COUNT, &mut rng);

        let parts: Vec<&str> = password.split('-').collect();
        assert_eq!(parts.len(), WORD_COUNT);
        for part in parts {
            assert!(part.chars().count() >= 5, "word too short: {part}");
            assert!(
                part.chars().all(|c| c.is_ascii_lowercase()),
                "unexpected characters in: {part}"
            );
            assert!(words.iter().any(|word| *word == part))
        }
    }
}
