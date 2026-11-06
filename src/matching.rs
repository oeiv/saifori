//! Deterministic, conservative matching of product names.
//! Similarity is a review hint, never permission to merge or delete a transaction.
use std::collections::HashSet;

pub fn normalize_name(input: &str) -> String {
    let mut out = String::new();
    let mut pending_space = false;
    for c in input.trim().chars().flat_map(char::to_lowercase) {
        let c = match c {
            // Frequent Latin accent variants; keep UTF-8 names in the UI.
            'á' | 'à' | 'â' | 'ä' | 'ã' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'ö' | 'õ' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            'ñ' => 'n',
            _ => c,
        };
        if c.is_alphanumeric() {
            if pending_space && !out.is_empty() {
                out.push(' ');
            }
            pending_space = false;
            out.push(c);
        } else {
            pending_space = true;
        }
    }
    out
}

fn edit_distance(a: &str, b: &str) -> usize {
    let left: Vec<char> = a.chars().collect();
    let right: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=right.len()).collect();
    for (i, x) in left.iter().enumerate() {
        let mut current = vec![i + 1; right.len() + 1];
        for (j, y) in right.iter().enumerate() {
            current[j + 1] = (prev[j + 1] + 1)
                .min(current[j] + 1)
                .min(prev[j] + usize::from(x != y));
        }
        prev = current;
    }
    prev[right.len()]
}

/// Conservative score for suggestions: return None when names are too dissimilar.
/// No automatic alias creation is performed here.
pub fn similarity(a: &str, b: &str) -> Option<u8> {
    let x = normalize_name(a);
    let y = normalize_name(b);
    if x.is_empty() || y.is_empty() {
        return None;
    }
    if x == y {
        return Some(100);
    }
    let lx = x.chars().count();
    let ly = y.chars().count();
    if lx.min(ly) < 4 || lx.max(ly) > 100 {
        return None;
    }
    let tokens_a: HashSet<_> = x.split_whitespace().collect();
    let tokens_b: HashSet<_> = y.split_whitespace().collect();
    if tokens_a.len() > 1 && tokens_a == tokens_b {
        return Some(98);
    }
    // Prevent substring collisions like "coffee" and "coffee beans".
    if tokens_a.len() != tokens_b.len() {
        return None;
    }
    let diff = lx.abs_diff(ly);
    if diff > 3 && diff * 5 > lx.max(ly) {
        return None;
    }
    let max_len = lx.max(ly);
    let score = (100 * (max_len - edit_distance(&x, &y)) / max_len) as u8;
    if score >= 80 {
        Some(score)
    } else {
        None
    }
}

pub fn validate_category_name(name: &str) -> Result<String, String> {
    let normalized = name.trim();
    if normalized.chars().count() < 2 || normalized.chars().count() > 60 {
        return Err("Category name must be between 2 and 60 characters.".into());
    }
    if normalized
        .chars()
        .any(|c| c.is_control() || matches!(c, '<' | '>' | '\\' | '/' | '"'))
    {
        return Err("Category contains unsupported characters.".into());
    }
    Ok(normalized.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn names_are_normalized_without_destroying_original_spelling() {
        assert_eq!(normalize_name("  Café---LATTE "), "cafe latte");
        assert_eq!(normalize_name("Piñata ÜBER"), "pinata uber");
        assert_eq!(normalize_name("Crème--brûlée"), "creme brulee");
    }
    #[test]
    fn fuzzy_matches_are_only_review_hints() {
        assert!(similarity("Coffe Latte", "Coffee Latte").unwrap() >= 80);
        assert!(similarity("Café con Leche", "Cafe con Leche").is_some());
        assert!(similarity("Coffee", "Coffee Beans").is_none());
        assert!(similarity("Rice", "Sugar").is_none());
    }
}
