/// Normalizes verbose taxon labels (e.g., truncating authorship info).
pub fn normalize_taxon_name(taxon_name: &str) -> String {
    taxon_name
        .split_whitespace()
        .take(2)
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trims_and_truncates() {
        assert_eq!(
            normalize_taxon_name("Vernonanthura patens (Kunth) H.Rob."),
            "Vernonanthura patens"
        );
        assert_eq!(normalize_taxon_name("Single"), "Single");
        assert_eq!(
            normalize_taxon_name("  Leading  and trailing  "),
            "Leading and"
        );
    }
}
