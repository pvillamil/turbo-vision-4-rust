// (C) 2025 - Enzo Lombardi

//! PictureValidator - validates and formats input using picture mask patterns.
//!
//! Port of Borland's `TPXPictureValidator` (TValidator.cc / validate.h),
//! including the full picture state machine.
//!
//! Mask characters (Borland semantics):
//! - `#` : digit (0-9)
//! - `?` : letter (a-z, A-Z)
//! - `&` : letter, forced to uppercase
//! - `@` : any character
//! - `!` : any character, forced to uppercase
//! - `;` : escapes the next character, making it a literal
//! - `*` : repetition — `*3#` means exactly three digits, `*#` means any
//!   number of digits
//! - `{}` : group (all members required)
//! - `[]` : optional group
//! - `,`  : alternative separator
//! - any other character is a literal; a typed space is replaced by the
//!   literal, and literal matching is case-insensitive with the mask's case
//!   winning
//!
//! Examples:
//! - `"(###) ###-####"` : phone number (555) 123-4567
//! - `"##/##/####"`     : date 12/25/2023
//! - `"&&&&-####"`      : code ABCD-1234 (letters uppercased)
//!
//! With auto-fill enabled (the default), literal characters are inserted
//! automatically while typing: entering "12" against `"##/##"` yields "12/".

use crate::views::validator::{Validator, ValidatorRef};
use std::cell::RefCell;
use std::rc::Rc;

/// Result codes of the picture state machine.
/// Matches Borland's `TPicResult` (validate.h).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PicResult {
    /// Input completely satisfies the mask.
    Complete,
    /// Input is a valid prefix of the mask.
    Incomplete,
    /// Input is empty.
    Empty,
    /// Input cannot match the mask.
    Error,
    /// The mask itself is malformed.
    Syntax,
    /// Input could be complete or continue (e.g. open-ended repetition).
    Ambiguous,
    /// Incomplete, and auto-fill must not run.
    IncompNoFill,
}

fn is_complete(r: PicResult) -> bool {
    matches!(r, PicResult::Complete | PicResult::Ambiguous)
}

fn is_incomplete(r: PicResult) -> bool {
    matches!(r, PicResult::Incomplete | PicResult::IncompNoFill)
}

/// The picture state machine. Port of the index/jndex-based algorithm in
/// Borland's TValidator.cc, operating on char vectors for UTF-8 safety.
struct PicMachine<'a> {
    pic: &'a [char],
    input: Vec<char>,
    /// Current position in the mask (`index` in Borland).
    index: usize,
    /// Current position in the input (`jndex` in Borland).
    jndex: usize,
}

impl<'a> PicMachine<'a> {
    fn new(pic: &'a [char], input: &str) -> Self {
        Self {
            pic,
            input: input.chars().collect(),
            index: 0,
            jndex: 0,
        }
    }

    /// Reads the mask character at `i`, or NUL past the end (mirrors C's
    /// string terminator reads).
    fn pic_at(&self, i: usize) -> char {
        self.pic.get(i).copied().unwrap_or('\0')
    }

    /// Consume one input character (possibly transformed).
    fn consume(&mut self, ch: char) {
        self.input[self.jndex] = ch;
        self.index += 1;
        self.jndex += 1;
    }

    /// Skip a character or a picture group. Matches `toGroupEnd`.
    fn to_group_end(&self, i: &mut usize, term_ch: usize) {
        let mut brk_level = 0i32;
        let mut brc_level = 0i32;
        loop {
            if *i == term_ch {
                return;
            }
            match self.pic_at(*i) {
                '[' => brk_level += 1,
                ']' => brk_level -= 1,
                '{' => brc_level += 1,
                '}' => brc_level -= 1,
                ';' => *i += 1,
                _ => {}
            }
            *i += 1;
            if brk_level == 0 && brc_level == 0 {
                return;
            }
        }
    }

    /// Find the next comma separator. Matches `skipToComma`.
    fn skip_to_comma(&mut self, term_ch: usize) -> bool {
        loop {
            let mut i = self.index;
            self.to_group_end(&mut i, term_ch);
            self.index = i;
            if self.index == term_ch || self.pic_at(self.index) == ',' {
                break;
            }
        }
        if self.pic_at(self.index) == ',' {
            self.index += 1;
        }
        self.index < term_ch
    }

    /// Calculate the end of the current group. Matches `calcTerm`.
    fn calc_term(&self, term_ch: usize) -> usize {
        let mut k = self.index;
        self.to_group_end(&mut k, term_ch);
        k
    }

    /// Process a `*` repetition. Matches `iteration`.
    fn iteration(&mut self, in_term: usize) -> PicResult {
        let mut itr: usize = 0;
        let mut rslt;

        self.index += 1; // Skip '*'

        while self.pic_at(self.index).is_ascii_digit() {
            itr = itr * 10 + (self.pic_at(self.index) as usize - '0' as usize);
            self.index += 1;
        }

        let k = self.index;
        let term_ch = self.calc_term(in_term);

        if itr != 0 {
            rslt = PicResult::Error;
            for _ in 1..=itr {
                self.index = k;
                rslt = self.process(term_ch);
                if !is_complete(rslt) {
                    if rslt == PicResult::Empty {
                        rslt = PicResult::Incomplete;
                    }
                    return rslt;
                }
            }
        } else {
            loop {
                self.index = k;
                rslt = self.process(term_ch);
                if rslt != PicResult::Complete {
                    break;
                }
            }
            if rslt == PicResult::Empty || rslt == PicResult::Error {
                self.index += 1;
                rslt = PicResult::Ambiguous;
            }
        }
        self.index = term_ch;
        rslt
    }

    /// Process a `{}` or `[]` group. Matches `group`.
    fn group(&mut self, in_term: usize) -> PicResult {
        let term_ch = self.calc_term(in_term);
        self.index += 1;
        let rslt = self.process(term_ch - 1);
        if !is_incomplete(rslt) {
            self.index = term_ch;
        }
        rslt
    }

    /// Matches `checkComplete`.
    fn check_complete(&self, mut rslt: PicResult, term_ch: usize) -> PicResult {
        let mut j = self.index;
        if is_incomplete(rslt) {
            // Skip optional pieces
            loop {
                match self.pic_at(j) {
                    '[' => self.to_group_end(&mut j, term_ch),
                    '*' => {
                        if !self.pic_at(j + 1).is_ascii_digit() {
                            j += 1;
                            self.to_group_end(&mut j, term_ch);
                        } else {
                            break;
                        }
                    }
                    _ => break,
                }
            }
            if j == term_ch {
                rslt = PicResult::Ambiguous;
            }
        }
        rslt
    }

    /// Matches `scan`.
    fn scan(&mut self, term_ch: usize) -> PicResult {
        let mut rslt = PicResult::Empty;

        while self.index != term_ch && self.pic_at(self.index) != ',' {
            if self.jndex >= self.input.len() {
                return self.check_complete(rslt, term_ch);
            }

            let ch = self.input[self.jndex];
            match self.pic_at(self.index) {
                '#' => {
                    if !ch.is_ascii_digit() {
                        return PicResult::Error;
                    }
                    self.consume(ch);
                }
                '?' => {
                    if !ch.is_ascii_alphabetic() {
                        return PicResult::Error;
                    }
                    self.consume(ch);
                }
                '&' => {
                    if !ch.is_ascii_alphabetic() {
                        return PicResult::Error;
                    }
                    self.consume(ch.to_ascii_uppercase());
                }
                '!' => {
                    self.consume(ch.to_ascii_uppercase());
                }
                '@' => {
                    self.consume(ch);
                }
                '*' => {
                    rslt = self.iteration(term_ch);
                    if !is_complete(rslt) {
                        return rslt;
                    }
                    if rslt == PicResult::Error {
                        rslt = PicResult::Ambiguous;
                    }
                }
                '{' => {
                    rslt = self.group(term_ch);
                    if !is_complete(rslt) {
                        return rslt;
                    }
                }
                '[' => {
                    rslt = self.group(term_ch);
                    if is_incomplete(rslt) {
                        return rslt;
                    }
                    if rslt == PicResult::Error {
                        rslt = PicResult::Ambiguous;
                    }
                }
                _ => {
                    // Literal (possibly `;`-escaped). Case-insensitive match;
                    // a typed space is replaced by the literal.
                    if self.pic_at(self.index) == ';' {
                        self.index += 1;
                    }
                    let lit = self.pic_at(self.index);
                    if lit.to_ascii_uppercase() != ch.to_ascii_uppercase() && ch != ' ' {
                        return PicResult::Error;
                    }
                    self.consume(lit);
                }
            }

            if rslt == PicResult::Ambiguous {
                rslt = PicResult::IncompNoFill;
            } else {
                rslt = PicResult::Incomplete;
            }
        }

        if rslt == PicResult::IncompNoFill {
            PicResult::Ambiguous
        } else {
            PicResult::Complete
        }
    }

    /// Matches `process`.
    fn process(&mut self, term_ch: usize) -> PicResult {
        let mut incomp = false;
        let mut old_i = self.index;
        let old_j = self.jndex;
        let mut incomp_i = 0;
        let mut incomp_j = 0;

        loop {
            let mut rslt = self.scan(term_ch);

            // Only accept completes if they make it farther in the input
            // stream than the last incomplete
            if rslt == PicResult::Complete && incomp && self.jndex < incomp_j {
                rslt = PicResult::Incomplete;
                self.jndex = incomp_j;
            }

            if rslt == PicResult::Error || rslt == PicResult::Incomplete {
                let mut r_process = rslt;
                if !incomp && rslt == PicResult::Incomplete {
                    incomp = true;
                    incomp_i = self.index;
                    incomp_j = self.jndex;
                }
                self.index = old_i;
                self.jndex = old_j;
                if !self.skip_to_comma(term_ch) {
                    if incomp {
                        r_process = PicResult::Incomplete;
                        self.index = incomp_i;
                        self.jndex = incomp_j;
                    }
                    return r_process;
                }
                old_i = self.index;
            } else {
                if rslt == PicResult::Complete && incomp {
                    return PicResult::Ambiguous;
                }
                return rslt;
            }
        }
    }
}

/// Picture mask validator for formatted input.
/// Port of Borland's `TPXPictureValidator`.
pub struct PictureValidator {
    /// Picture mask string
    mask: String,
    /// Whether to auto-fill literal characters as the user types (voFill)
    auto_format: bool,
}

impl PictureValidator {
    /// Create a new picture validator with the given mask (auto-fill on).
    ///
    /// # Example
    /// ```
    /// use turbo_vision::views::picture_validator::PictureValidator;
    ///
    /// // Phone number mask
    /// let validator = PictureValidator::new("(###) ###-####");
    ///
    /// // Date mask
    /// let validator = PictureValidator::new("##/##/####");
    /// ```
    pub fn new(mask: &str) -> Self {
        PictureValidator {
            mask: mask.to_string(),
            auto_format: true,
        }
    }

    /// Create a new picture validator without auto-fill.
    pub fn new_no_format(mask: &str) -> Self {
        PictureValidator {
            mask: mask.to_string(),
            auto_format: false,
        }
    }

    /// Get the mask string
    pub fn mask(&self) -> &str {
        &self.mask
    }

    /// Set whether to auto-fill literals while typing
    pub fn set_auto_format(&mut self, auto_format: bool) {
        self.auto_format = auto_format;
    }

    /// Validates the mask's `{}`/`[]` nesting and trailing `;`.
    /// Matches Borland's `syntaxCheck`.
    fn syntax_check(&self) -> bool {
        let pic: Vec<char> = self.mask.chars().collect();
        if pic.is_empty() || *pic.last().unwrap() == ';' {
            return false;
        }
        let mut brk = 0i32;
        let mut brc = 0i32;
        let mut i = 0;
        while i < pic.len() {
            match pic[i] {
                '[' => brk += 1,
                ']' => brk -= 1,
                '{' => brc += 1,
                '}' => brc -= 1,
                ';' => i += 1,
                _ => {}
            }
            i += 1;
        }
        brk == 0 && brc == 0
    }

    /// Runs the picture machine over `input`. Returns the result code and the
    /// (possibly transformed) input: uppercase forcing from `&`/`!`, literal
    /// case correction, and — when `auto_fill` is set and the input is an
    /// incomplete prefix — trailing literal characters appended.
    ///
    /// Matches Borland's `TPXPictureValidator::picture`.
    pub fn picture(&self, input: &str, auto_fill: bool) -> (PicResult, String) {
        if !self.syntax_check() {
            return (PicResult::Syntax, input.to_string());
        }
        if input.is_empty() {
            return (PicResult::Empty, String::new());
        }

        let pic: Vec<char> = self.mask.chars().collect();
        let mut m = PicMachine::new(&pic, input);
        let mut rslt = m.process(pic.len());

        if rslt != PicResult::Error && m.jndex < m.input.len() {
            rslt = PicResult::Error;
        }

        if rslt == PicResult::Incomplete && auto_fill {
            let mut reprocess = false;
            while m.index < pic.len() && !"#?&!@*{}[],".contains(m.pic_at(m.index)) {
                if m.pic_at(m.index) == ';' {
                    m.index += 1;
                }
                let lit = m.pic_at(m.index);
                m.input.push(lit);
                m.index += 1;
                reprocess = true;
            }
            if reprocess {
                let filled: String = m.input.iter().collect();
                m = PicMachine::new(&pic, &filled);
                rslt = m.process(pic.len());
            }
        }

        let out: String = m.input.iter().collect();
        let rslt = match rslt {
            PicResult::Ambiguous => PicResult::Complete,
            PicResult::IncompNoFill => PicResult::Incomplete,
            other => other,
        };
        (rslt, out)
    }
}

impl Validator for PictureValidator {
    fn is_valid(&self, input: &str) -> bool {
        if input.is_empty() {
            return true; // Empty is valid (emptiness is enforced elsewhere)
        }
        self.picture(input, false).0 == PicResult::Complete
    }

    fn is_valid_input(&self, input: &str, _append: bool) -> bool {
        if input.is_empty() {
            return true;
        }
        // Matches Borland: isValidInput returns picture(...) != prError
        self.picture(input, false).0 != PicResult::Error
    }

    fn complete(&self, input: &str) -> Option<String> {
        if input.is_empty() {
            return None;
        }
        let (rslt, transformed) = self.picture(input, self.auto_format);
        if rslt != PicResult::Error && rslt != PicResult::Syntax && transformed != input {
            Some(transformed)
        } else {
            None
        }
    }

    fn error(&self) {
        // In a full implementation, this would show a message box
        // For now, just a no-op (the InputLine will handle visual feedback)
    }

    fn valid(&self, input: &str) -> bool {
        if self.is_valid(input) {
            true
        } else {
            self.error();
            false
        }
    }
}

/// Helper function to create a ValidatorRef for a PictureValidator
pub fn picture_validator(mask: &str) -> ValidatorRef {
    Rc::new(RefCell::new(PictureValidator::new(mask)))
}

/// Builder for creating picture validators with a fluent API.
///
/// # Examples
///
/// ```ignore
/// use turbo_vision::views::picture_validator::PictureValidatorBuilder;
///
/// // Create a phone number validator with auto-formatting
/// let validator = PictureValidatorBuilder::new()
///     .mask("(###) ###-####")
///     .build();
///
/// // Create a date validator without auto-formatting
/// let validator = PictureValidatorBuilder::new()
///     .mask("##/##/####")
///     .auto_format(false)
///     .build();
/// ```
pub struct PictureValidatorBuilder {
    mask: Option<String>,
    auto_format: bool,
}

impl PictureValidatorBuilder {
    /// Creates a new PictureValidatorBuilder with default values.
    pub fn new() -> Self {
        Self {
            mask: None,
            auto_format: true,
        }
    }

    /// Sets the picture mask pattern (required).
    ///
    /// Mask characters (Borland TPXPictureValidator):
    /// - `#` : digit
    /// - `?` : letter
    /// - `&` : letter, uppercased
    /// - `@` : any character
    /// - `!` : any character, uppercased
    /// - `;` : escape next character to a literal
    /// - `*`, `{}`, `[]`, `,` : repetition, groups, optional groups,
    ///   alternatives
    /// - other : literal characters
    #[must_use]
    pub fn mask(mut self, mask: impl Into<String>) -> Self {
        self.mask = Some(mask.into());
        self
    }

    /// Sets whether to auto-fill literal characters (default: true).
    #[must_use]
    pub fn auto_format(mut self, auto_format: bool) -> Self {
        self.auto_format = auto_format;
        self
    }

    /// Builds the PictureValidator.
    ///
    /// # Panics
    ///
    /// Panics if required fields (mask) are not set.
    pub fn build(self) -> PictureValidator {
        let mask = self.mask.expect("PictureValidator mask must be set");

        PictureValidator {
            mask,
            auto_format: self.auto_format,
        }
    }

    /// Builds the PictureValidator as a ValidatorRef.
    pub fn build_ref(self) -> ValidatorRef {
        Rc::new(RefCell::new(self.build()))
    }
}

impl Default for PictureValidatorBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_phone_number_mask() {
        let validator = PictureValidator::new("(###) ###-####");

        // Valid complete input
        assert!(validator.is_valid("(555) 123-4567"));

        // Invalid - wrong format
        assert!(!validator.is_valid("555-123-4567"));
        assert!(!validator.is_valid("(abc) def-ghij"));
    }

    #[test]
    fn test_date_mask() {
        let validator = PictureValidator::new("##/##/####");

        // Valid dates
        assert!(validator.is_valid("12/25/2023"));
        assert!(validator.is_valid("01/01/2000"));

        // Invalid dates
        assert!(!validator.is_valid("12-25-2023")); // Wrong separator
        assert!(!validator.is_valid("1/1/2023")); // Missing leading zeros
    }

    #[test]
    fn test_digit_mask_char() {
        let validator = PictureValidator::new("###");
        assert!(validator.is_valid("123"));
        assert!(!validator.is_valid("12a"));
        assert!(validator.is_valid_input("12", true));
        assert!(!validator.is_valid_input("a", true));
    }

    #[test]
    fn test_letter_mask_char() {
        // `?` is letter in Borland semantics
        let validator = PictureValidator::new("???");
        assert!(validator.is_valid("abc"));
        assert!(validator.is_valid("XYZ"));
        assert!(!validator.is_valid("ab1"));
    }

    #[test]
    fn test_uppercase_letter_mask_char() {
        // `&` is letter forced to uppercase
        let validator = PictureValidator::new("&&&");
        assert!(!validator.is_valid_input("a1", true));
        let (rslt, out) = validator.picture("abc", false);
        assert_eq!(rslt, PicResult::Complete);
        assert_eq!(out, "ABC");
    }

    #[test]
    fn test_any_char_mask_char() {
        // `@` is ANY character in Borland semantics (not just alpha)
        let validator = PictureValidator::new("@@@");
        assert!(validator.is_valid("a1!"));
        assert!(validator.is_valid("..."));
    }

    #[test]
    fn test_uppercase_any_mask_char() {
        // `!` is ANY character forced to uppercase
        let validator = PictureValidator::new("!!!");
        let (rslt, out) = validator.picture("a1z", false);
        assert_eq!(rslt, PicResult::Complete);
        assert_eq!(out, "A1Z");
    }

    #[test]
    fn test_semicolon_escape() {
        // `;#` is a literal '#', not a digit slot
        let validator = PictureValidator::new(";##");
        assert!(validator.is_valid("#5"));
        assert!(!validator.is_valid("55"));

        // `;;` is a literal ';'
        let validator = PictureValidator::new(";;#");
        assert!(validator.is_valid(";5"));

        // Trailing ';' is a syntax error
        let validator = PictureValidator::new("##;");
        assert_eq!(validator.picture("12", false).0, PicResult::Syntax);
    }

    #[test]
    fn test_literal_case_insensitive_and_space_fill() {
        let validator = PictureValidator::new("ab#");
        // Literal match is case-insensitive; mask's case wins
        let (rslt, out) = validator.picture("AB1", false);
        assert_eq!(rslt, PicResult::Complete);
        assert_eq!(out, "ab1");
        // A typed space is replaced by the literal
        let (rslt, out) = validator.picture("  1", false);
        assert_eq!(rslt, PicResult::Complete);
        assert_eq!(out, "ab1");
    }

    #[test]
    fn test_auto_fill_literals() {
        let validator = PictureValidator::new("##/##");
        let (rslt, out) = validator.picture("12", true);
        assert_eq!(rslt, PicResult::Incomplete);
        assert_eq!(out, "12/");

        // Without fill, no literal is appended
        let (_, out) = validator.picture("12", false);
        assert_eq!(out, "12");
    }

    #[test]
    fn test_complete_trait_method() {
        let validator = PictureValidator::new("##/##");
        assert_eq!(validator.complete("12").as_deref(), Some("12/"));
        assert_eq!(validator.complete("1"), None); // nothing to fill
        // Uppercase forcing surfaces through complete() too
        let validator = PictureValidator::new("!!");
        assert_eq!(validator.complete("ab").as_deref(), Some("AB"));
    }

    #[test]
    fn test_repetition_fixed_count() {
        // *3# = exactly three digits
        let validator = PictureValidator::new("*3#");
        assert!(validator.is_valid("123"));
        assert!(!validator.is_valid("12"));
        assert!(!validator.is_valid("1234"));
    }

    #[test]
    fn test_repetition_open_ended() {
        // *# = any number of digits
        let validator = PictureValidator::new("*#");
        assert!(validator.is_valid("1"));
        assert!(validator.is_valid("123456"));
        assert!(!validator.is_valid("12a"));
    }

    #[test]
    fn test_optional_group() {
        // [-]### = optional leading minus, then three digits
        let validator = PictureValidator::new("[-]###");
        assert!(validator.is_valid("123"));
        assert!(validator.is_valid("-123"));
        assert!(!validator.is_valid("+123"));
    }

    #[test]
    fn test_alternatives() {
        let validator = PictureValidator::new("{red,green,blue}");
        assert!(validator.is_valid("red"));
        assert!(validator.is_valid("blue"));
        assert!(!validator.is_valid("pink"));
    }

    #[test]
    fn test_partial_input_validation() {
        let validator = PictureValidator::new("(###) ###-####");

        // Partial inputs should be valid during typing
        assert!(validator.is_valid_input("(5", false));
        assert!(validator.is_valid_input("(55", false));
        assert!(validator.is_valid_input("(555", false));
        assert!(validator.is_valid_input("(555) ", false));
        assert!(validator.is_valid_input("(555) 1", false));
        // But a digit where the '(' literal belongs is rejected
        assert!(!validator.is_valid_input("5", false));
    }

    #[test]
    fn test_empty_input() {
        let validator = PictureValidator::new("##/##/####");
        assert!(validator.is_valid("")); // Empty is valid
    }

    #[test]
    fn test_validator_trait() {
        let validator = PictureValidator::new("(###) ###-####");
        assert!(validator.valid("(555) 123-4567"));
        assert!(!validator.valid("invalid"));
    }

    #[test]
    fn test_picture_validator_builder() {
        let validator = PictureValidatorBuilder::new().mask("##/##/####").build();

        assert!(validator.is_valid("12/25/2023"));
        assert_eq!(validator.mask(), "##/##/####");
    }

    #[test]
    fn test_picture_validator_builder_no_format() {
        let validator = PictureValidatorBuilder::new()
            .mask("(###) ###-####")
            .auto_format(false)
            .build();

        assert!(validator.is_valid("(555) 123-4567"));
    }
}
