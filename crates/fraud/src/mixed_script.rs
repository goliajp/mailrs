//! A word that mixes Latin letters with letters of a script that has
//! Latin look-alikes.
//!
//! Reported 2026-11-12: `ᏚhiеldеdFilе - ꓖoliа`, a Cherokee `Ꮪ`, Cyrillic
//! `е` and `а` and a Lisu `ꓖ` among Latin letters, so the name reads as
//! `ShieldedFile - Golia` and matches no filter that compares text. In
//! 41,752 external production messages it was the only display name with
//! a word written in two such scripts; Cyrillic, Lisu and Cherokee
//! letters appeared in no other display name at all.
//!
//! A name written in one of those scripts is a name. A Latin word with a
//! letter from one of them is a disguise: no language spells a word that
//! way. Words are split on anything that is not a letter, so `IT-отдел`
//! and `Яндекс.Go` are two words each.

/// Scripts that hold letters drawn like Latin ones.
fn look_alike_script(c: char) -> bool {
    matches!(c as u32,
        0x0370..=0x03FF   // Greek
        | 0x0400..=0x052F // Cyrillic and its supplement
        | 0x0530..=0x058F // Armenian
        | 0x13A0..=0x13FF // Cherokee
        | 0xA4D0..=0xA4FF // Lisu
        | 0xAB70..=0xABBF // Cherokee supplement
        | 0x104B0..=0x104FF // Osage
    )
}

/// Whether some word of `name` has both an ASCII Latin letter and a letter
/// of a look-alike script.
#[must_use]
pub fn mixes_scripts(name: &str) -> bool {
    name.split(|c: char| !c.is_alphabetic()).any(|word| {
        word.chars().any(|c| c.is_ascii_alphabetic()) && word.chars().any(look_alike_script)
    })
}

#[cfg(test)]
mod tests {
    use super::mixes_scripts;

    #[test]
    fn a_latin_word_with_look_alike_letters_mixes() {
        assert!(mixes_scripts("ᏚhiеldеdFilе - ꓖoliа"));
        // not in the corpus: one Cyrillic `о` in an English brand, Greek
        // `α`, an Armenian `օ`
        assert!(mixes_scripts("Micrоsoft Account"));
        assert!(mixes_scripts("Pαypal"));
        assert!(mixes_scripts("Gօogle"));
    }

    #[test]
    fn names_in_one_script_each_do_not_mix() {
        for name in [
            "Иван Петров",
            "Ivan Петров",
            "IT-отдел",
            "Яндекс.Go",
            "Γιώργος Papadopoulos",
            "李好 Li Hao",
            "Amázon",
            "Qualcomm Esignature via Docusign",
            "",
        ] {
            assert!(!mixes_scripts(name), "{name}");
        }
    }
}
