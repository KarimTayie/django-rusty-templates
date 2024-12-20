use miette::{Diagnostic, SourceSpan};
use thiserror::Error;

use crate::lex::common::{
    lex_numeric, lex_text, lex_translated, lex_variable_argument, LexerError,
};
use crate::lex::tag::TagParts;

#[derive(Debug, PartialEq)]
pub enum UrlTokenType {
    As,
    Numeric,
    Text,
    TranslatedText,
    Variable,
}

#[derive(Debug, PartialEq)]
pub struct UrlToken {
    pub at: (usize, usize),
    pub token_type: UrlTokenType,
    pub kwarg: Option<(usize, usize)>,
}

#[derive(Error, Debug, Diagnostic, PartialEq, Eq)]
pub enum UrlLexerError {
    #[error(transparent)]
    #[diagnostic(transparent)]
    LexerError(#[from] LexerError),
    #[error("")]
    IncompleteKeywordArgument {
        #[label("here")]
        at: SourceSpan,
    },
}

pub struct UrlLexer<'t> {
    rest: &'t str,
    byte: usize,
}

impl<'t> UrlLexer<'t> {
    pub fn new(template: &'t str, parts: TagParts) -> Self {
        let (start, len) = parts.at;
        Self {
            rest: &template[start..start + len],
            byte: start,
        }
    }

    fn lex_numeric(&mut self, kwarg: Option<(usize, usize)>) -> UrlToken {
        let (at, byte, rest) = lex_numeric(self.byte, self.rest);
        self.rest = rest;
        self.byte = byte;
        UrlToken {
            at,
            token_type: UrlTokenType::Numeric,
            kwarg,
        }
    }

    fn lex_text(
        &mut self,
        chars: &mut std::str::Chars,
        end: char,
        kwarg: Option<(usize, usize)>,
    ) -> Result<UrlToken, UrlLexerError> {
        match lex_text(self.byte, self.rest, chars, end) {
            Ok((at, byte, rest)) => {
                self.rest = rest;
                self.byte = byte;
                Ok(UrlToken {
                    token_type: UrlTokenType::Text,
                    at,
                    kwarg,
                })
            }
            Err(e) => {
                self.rest = "";
                Err(e.into())
            }
        }
    }

    fn lex_translated(
        &mut self,
        chars: &mut std::str::Chars,
        kwarg: Option<(usize, usize)>,
    ) -> Result<UrlToken, UrlLexerError> {
        match lex_translated(self.byte, self.rest, chars) {
            Ok((at, byte, rest)) => {
                self.rest = rest;
                self.byte = byte;
                Ok(UrlToken {
                    token_type: UrlTokenType::TranslatedText,
                    at,
                    kwarg,
                })
            }
            Err(e) => {
                self.rest = "";
                Err(e.into())
            }
        }
    }

    fn lex_variable(&mut self, kwarg: Option<(usize, usize)>) -> Result<UrlToken, UrlLexerError> {
        let at = match lex_variable_argument(self.byte, self.rest) {
            Ok((at, byte, rest)) => {
                self.byte = byte;
                self.rest = rest;
                at
            }
            Err(e) => {
                self.rest = "";
                return Err(e.into());
            }
        };
        if kwarg.is_some() {
            return Ok(UrlToken {
                token_type: UrlTokenType::Variable,
                at,
                kwarg,
            });
        }
        let mut chars = self.rest.chars();
        match chars.next() {
            Some('=') => {
                let next = match chars.next() {
                    None => {
                        self.rest = "";
                        let at = (at.0, at.1 + 1).into();
                        return Err(UrlLexerError::IncompleteKeywordArgument { at });
                    }
                    Some(next) => next,
                };
                self.byte += 1;
                self.rest = &self.rest[1..];
                match next {
                    '_' => {
                        if let Some('(') = chars.next() {
                            self.lex_translated(&mut chars, Some(at))
                        } else {
                            self.lex_variable(Some(at))
                        }
                    }
                    '"' => self.lex_text(&mut chars, '"', Some(at)),
                    '\'' => self.lex_text(&mut chars, '\'', Some(at)),
                    '0'..='9' | '-' => Ok(self.lex_numeric(Some(at))),
                    _ => self.lex_variable(Some(at)),
                }
            }
            _ => Ok(UrlToken {
                token_type: UrlTokenType::Variable,
                at,
                kwarg: None,
            }),
        }
    }

    fn lex_remainder(
        &mut self,
        token: Result<UrlToken, UrlLexerError>,
    ) -> Result<UrlToken, UrlLexerError> {
        let remainder = self
            .rest
            .find(char::is_whitespace)
            .unwrap_or(self.rest.len());
        match remainder {
            0 => token,
            n => {
                self.rest = "";
                let at = (self.byte, n).into();
                let err = LexerError::InvalidRemainder { at };
                Err(err.into())
            }
        }
    }
}

impl Iterator for UrlLexer<'_> {
    type Item = Result<UrlToken, UrlLexerError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.rest.is_empty() {
            return None;
        }

        let mut chars = self.rest.chars();
        let token = match chars.next().unwrap() {
            '_' => {
                if let Some('(') = chars.next() {
                    self.lex_translated(&mut chars, None)
                } else {
                    self.lex_variable(None)
                }
            }
            '"' => self.lex_text(&mut chars, '"', None),
            '\'' => self.lex_text(&mut chars, '\'', None),
            '0'..='9' | '-' => Ok(self.lex_numeric(None)),
            _ => self.lex_variable(None),
        };
        Some(self.lex_remainder(token))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lex_url_name_text() {
        let template = "{% url 'foo' %}";
        let parts = TagParts { at: (7, 5) };
        let mut lexer = UrlLexer::new(template, parts);
        let tokens: Vec<_> = lexer.collect();
        let name = UrlToken {
            at: (7, 5),
            token_type: UrlTokenType::Text,
            kwarg: None,
        };
        assert_eq!(tokens, vec![Ok(name)]);
    }

    #[test]
    fn test_lex_url_name_text_double_quotes() {
        let template = "{% url \"foo\" %}";
        let parts = TagParts { at: (7, 5) };
        let mut lexer = UrlLexer::new(template, parts);
        let tokens: Vec<_> = lexer.collect();
        let name = UrlToken {
            at: (7, 5),
            token_type: UrlTokenType::Text,
            kwarg: None,
        };
        assert_eq!(tokens, vec![Ok(name)]);
    }

    #[test]
    fn test_lex_url_name_text_incomplete() {
        let template = "{% url 'foo %}";
        let parts = TagParts { at: (7, 4) };
        let mut lexer = UrlLexer::new(template, parts);
        let error = lexer.next().unwrap().unwrap_err();
        assert_eq!(
            error,
            LexerError::IncompleteString { at: (7, 4).into() }.into()
        );
    }

    #[test]
    fn test_lex_url_name_variable() {
        let template = "{% url foo %}";
        let parts = TagParts { at: (7, 3) };
        let mut lexer = UrlLexer::new(template, parts);
        let tokens: Vec<_> = lexer.collect();
        let name = UrlToken {
            at: (7, 3),
            token_type: UrlTokenType::Variable,
            kwarg: None,
        };
        assert_eq!(tokens, vec![Ok(name)]);
    }

    #[test]
    fn test_lex_url_name_invalid_variable() {
        let template = "{% url _foo %}";
        let parts = TagParts { at: (7, 4) };
        let mut lexer = UrlLexer::new(template, parts);
        let error = lexer.next().unwrap().unwrap_err();
        assert_eq!(
            error,
            LexerError::InvalidVariableName { at: (7, 4).into() }.into()
        );
    }

    #[test]
    fn test_lex_url_name_translated() {
        let template = "{% url _('foo') %}";
        let parts = TagParts { at: (7, 8) };
        let mut lexer = UrlLexer::new(template, parts);
        let tokens: Vec<_> = lexer.collect();
        let name = UrlToken {
            at: (7, 8),
            token_type: UrlTokenType::TranslatedText,
            kwarg: None,
        };
        assert_eq!(tokens, vec![Ok(name)]);
    }

    #[test]
    fn test_lex_url_name_translated_incomplete() {
        let template = "{% url _('foo' %}";
        let parts = TagParts { at: (7, 7) };
        let mut lexer = UrlLexer::new(template, parts);
        let error = lexer.next().unwrap().unwrap_err();
        assert_eq!(
            error,
            LexerError::IncompleteTranslatedString { at: (7, 7).into() }.into()
        );
    }

    #[test]
    fn test_lex_url_name_numeric() {
        let template = "{% url 5 %}";
        let parts = TagParts { at: (7, 1) };
        let mut lexer = UrlLexer::new(template, parts);
        let tokens: Vec<_> = lexer.collect();
        let name = UrlToken {
            at: (7, 1),
            token_type: UrlTokenType::Numeric,
            kwarg: None,
        };
        assert_eq!(tokens, vec![Ok(name)]);
    }

    #[test]
    fn test_lex_url_name_text_kwarg() {
        let template = "{% url name='foo' %}";
        let parts = TagParts { at: (7, 10) };
        let mut lexer = UrlLexer::new(template, parts);
        let tokens: Vec<_> = lexer.collect();
        let name = UrlToken {
            at: (12, 5),
            token_type: UrlTokenType::Text,
            kwarg: Some((7, 4)),
        };
        assert_eq!(tokens, vec![Ok(name)]);
    }

    #[test]
    fn test_lex_url_name_text_kwarg_double_quotes() {
        let template = "{% url name=\"foo\" %}";
        let parts = TagParts { at: (7, 10) };
        let mut lexer = UrlLexer::new(template, parts);
        let tokens: Vec<_> = lexer.collect();
        let name = UrlToken {
            at: (12, 5),
            token_type: UrlTokenType::Text,
            kwarg: Some((7, 4)),
        };
        assert_eq!(tokens, vec![Ok(name)]);
    }

    #[test]
    fn test_lex_url_name_variable_kwarg() {
        let template = "{% url name=foo %}";
        let parts = TagParts { at: (7, 8) };
        let mut lexer = UrlLexer::new(template, parts);
        let tokens: Vec<_> = lexer.collect();
        let name = UrlToken {
            at: (12, 3),
            token_type: UrlTokenType::Variable,
            kwarg: Some((7, 4)),
        };
        assert_eq!(tokens, vec![Ok(name)]);
    }

    #[test]
    fn test_lex_url_name_invalid_variable_kwarg() {
        let template = "{% url name=_foo %}";
        let parts = TagParts { at: (7, 9) };
        let mut lexer = UrlLexer::new(template, parts);
        let error = lexer.next().unwrap().unwrap_err();
        assert_eq!(
            error,
            LexerError::InvalidVariableName { at: (12, 4).into() }.into()
        );
    }

    #[test]
    fn test_lex_url_name_translated_kwarg() {
        let template = "{% url name=_('foo') %}";
        let parts = TagParts { at: (7, 13) };
        let mut lexer = UrlLexer::new(template, parts);
        let tokens: Vec<_> = lexer.collect();
        let name = UrlToken {
            at: (12, 8),
            token_type: UrlTokenType::TranslatedText,
            kwarg: Some((7, 4)),
        };
        assert_eq!(tokens, vec![Ok(name)]);
    }

    #[test]
    fn test_lex_url_name_numeric_kwarg() {
        let template = "{% url name=5 %}";
        let parts = TagParts { at: (7, 6) };
        let mut lexer = UrlLexer::new(template, parts);
        let tokens: Vec<_> = lexer.collect();
        let name = UrlToken {
            at: (12, 1),
            token_type: UrlTokenType::Numeric,
            kwarg: Some((7, 4)),
        };
        assert_eq!(tokens, vec![Ok(name)]);
    }

    #[test]
    fn test_lex_url_incomplete_kwarg() {
        let template = "{% url name= %}";
        let parts = TagParts { at: (7, 5) };
        let mut lexer = UrlLexer::new(template, parts);
        let error = lexer.next().unwrap().unwrap_err();
        assert_eq!(
            error,
            UrlLexerError::IncompleteKeywordArgument { at: (7, 5).into() }
        );
    }

    #[test]
    fn test_lex_url_invalid_remainder() {
        let template = "{% url foo'remainder %}";
        let parts = TagParts { at: (7, 13) };
        let mut lexer = UrlLexer::new(template, parts);
        let error = lexer.next().unwrap().unwrap_err();
        assert_eq!(
            error,
            LexerError::InvalidRemainder {
                at: (10, 10).into()
            }
            .into()
        );
    }

    #[test]
    fn test_lex_url_kwarg_invalid_remainder() {
        let template = "{% url name=foo=remainder %}";
        let parts = TagParts { at: (7, 18) };
        let mut lexer = UrlLexer::new(template, parts);
        let error = lexer.next().unwrap().unwrap_err();
        assert_eq!(
            error,
            LexerError::InvalidRemainder {
                at: (15, 10).into()
            }
            .into()
        );
    }
}
