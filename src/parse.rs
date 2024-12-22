use miette::{Diagnostic, SourceSpan};
use num_bigint::BigInt;
use thiserror::Error;

use crate::lex::core::{Lexer, TokenType};
use crate::lex::tag::{lex_tag, TagLexerError, TagParts};
use crate::lex::url::{UrlLexer, UrlLexerError, UrlToken, UrlTokenType};
use crate::lex::variable::{
    lex_variable, Argument as ArgumentToken, ArgumentType as ArgumentTokenType, VariableLexerError,
};
use crate::lex::START_TAG_LEN;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Variable {
    at: (usize, usize),
}

impl<'t> Variable {
    pub fn new(at: (usize, usize)) -> Self {
        Self { at }
    }

    fn content(&self, template: &'t str) -> &'t str {
        let (start, len) = self.at;
        &template[start..start + len]
    }

    pub fn parts(&self, template: &'t str) -> impl Iterator<Item = &'t str> {
        let variable = self.content(template);
        variable.split(".")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Text {
    at: (usize, usize),
}

impl<'t> Text {
    pub fn new(at: (usize, usize)) -> Self {
        Self { at }
    }

    pub fn content(&self, template: &'t str) -> &'t str {
        let (start, len) = self.at;
        &template[start..start + len]
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ArgumentType {
    Variable(Variable),
    Text(Text),
    TranslatedText(Text),
    Int(BigInt),
    Float(f64),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Argument {
    pub at: (usize, usize),
    pub argument_type: ArgumentType,
}

impl ArgumentToken {
    fn parse(&self, template: &'_ str) -> Result<Argument, ParseError> {
        Ok(Argument {
            at: self.at,
            argument_type: match self.argument_type {
                ArgumentTokenType::Variable => ArgumentType::Variable(Variable::new(self.at)),
                ArgumentTokenType::Text => ArgumentType::Text(Text::new(self.content_at())),
                ArgumentTokenType::Numeric => match self.content(template).parse::<BigInt>() {
                    Ok(n) => ArgumentType::Int(n),
                    Err(_) => match self.content(template).parse::<f64>() {
                        Ok(f) => ArgumentType::Float(f),
                        Err(_) => return Err(ParseError::InvalidNumber { at: self.at.into() }),
                    },
                },
                ArgumentTokenType::TranslatedText => {
                    ArgumentType::TranslatedText(Text::new(self.content_at()))
                }
            },
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum TagElement {
    Int(BigInt),
    Float(f64),
    Text(Text),
    TranslatedText(Text),
    Variable(Variable),
    Filter(Box<Filter>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum FilterType {
    Default(Argument),
    External(Option<Argument>),
    Lower,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Filter {
    at: (usize, usize),
    pub left: TagElement,
    pub filter: FilterType,
}

impl Filter {
    pub fn new(
        template: &str,
        at: (usize, usize),
        left: TagElement,
        right: Option<Argument>,
    ) -> Result<Self, ParseError> {
        let (start, len) = at;
        let filter = match &template[start..start + len] {
            "default" => match right {
                Some(right) => FilterType::Default(right),
                None => return Err(ParseError::MissingArgument { at: at.into() }),
            },
            "lower" => match right {
                Some(right) => {
                    return Err(ParseError::UnexpectedArgument {
                        at: right.at.into(),
                    })
                }
                None => FilterType::Lower,
            },
            _ => FilterType::External(right),
        };
        Ok(Self { at, left, filter })
    }
}

impl UrlToken {
    fn parse(&self, template: &str) -> Result<TagElement, ParseError> {
        let content_at = self.content_at();
        let (start, len) = content_at;
        let content = &template[start..start + len];
        match self.token_type {
            UrlTokenType::Numeric => match content.parse::<BigInt>() {
                Ok(n) => Ok(TagElement::Int(n)),
                Err(_) => match content.parse::<f64>() {
                    Ok(f) => Ok(TagElement::Float(f)),
                    Err(_) => Err(ParseError::InvalidNumber { at: self.at.into() }),
                },
            },
            UrlTokenType::Text => Ok(TagElement::Text(Text::new(content_at))),
            UrlTokenType::TranslatedText => Ok(TagElement::TranslatedText(Text::new(content_at))),
            UrlTokenType::Variable => parse_variable(template, content, content_at, start),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Url {
    view_name: TagElement,
    args: Vec<TagElement>,
    kwargs: Vec<(String, TagElement)>,
    variable: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Tag {
    Url(Url),
}

#[derive(Clone, Debug, PartialEq)]
pub enum TokenTree {
    Text(Text),
    TranslatedText(Text),
    Tag(Tag),
    Variable(Variable),
    Filter(Box<Filter>),
}

impl From<TagElement> for TokenTree {
    fn from(tag_element: TagElement) -> Self {
        match tag_element {
            TagElement::Text(text) => Self::Text(text),
            TagElement::TranslatedText(text) => Self::TranslatedText(text),
            TagElement::Variable(variable) => Self::Variable(variable),
            TagElement::Filter(filter) => Self::Filter(filter),
            TagElement::Int(_) => todo!(),
            TagElement::Float(_) => todo!(),
        }
    }
}

#[derive(Error, Debug, Diagnostic, PartialEq, Eq)]
pub enum ParseError {
    #[error("Empty block tag")]
    EmptyTag {
        #[label("here")]
        at: SourceSpan,
    },
    #[error("Empty variable tag")]
    EmptyVariable {
        #[label("here")]
        at: SourceSpan,
    },
    #[error("Expected an argument")]
    MissingArgument {
        #[label("here")]
        at: SourceSpan,
    },
    #[error(transparent)]
    #[diagnostic(transparent)]
    BlockError(#[from] TagLexerError),
    #[error(transparent)]
    #[diagnostic(transparent)]
    UrlLexerError(#[from] UrlLexerError),
    #[error(transparent)]
    #[diagnostic(transparent)]
    VariableError(#[from] VariableLexerError),
    #[error("Invalid numeric literal")]
    InvalidNumber {
        #[label("here")]
        at: SourceSpan,
    },
    #[error("Cannot mix arguments and keyword arguments")]
    MixedArgsKwargs {
        #[label("here")]
        at: SourceSpan,
    },
    #[error("'url' view name must be a string or variable, not a number")]
    NumericUrlName {
        #[label("here")]
        at: SourceSpan,
    },
    #[error("Expected an argument")]
    UnexpectedArgument {
        #[label("here")]
        at: SourceSpan,
    },
    #[error("'url' takes at least one argument, a URL pattern name")]
    UrlTagNoArguments {
        #[label("here")]
        at: SourceSpan,
    },
}

fn parse_variable(
    template: &str,
    variable: &str,
    at: (usize, usize),
    start: usize,
) -> Result<TagElement, ParseError> {
    let (variable_token, filter_lexer) = match lex_variable(variable, start)? {
        None => return Err(ParseError::EmptyVariable { at: at.into() }),
        Some(t) => t,
    };
    let mut var = TagElement::Variable(Variable::new(variable_token.at));
    for filter_token in filter_lexer {
        let filter_token = filter_token?;
        let argument = match filter_token.argument {
            None => None,
            Some(ref a) => Some(a.parse(template)?),
        };
        let filter = Filter::new(template, filter_token.at, var, argument)?;
        var = TagElement::Filter(Box::new(filter));
    }
    Ok(var)
}

pub struct Parser<'t> {
    template: &'t str,
    lexer: Lexer<'t>,
}

impl<'t> Parser<'t> {
    pub fn new(template: &'t str) -> Self {
        Self {
            template,
            lexer: Lexer::new(template),
        }
    }

    pub fn parse(&mut self) -> Result<Vec<TokenTree>, ParseError> {
        let mut nodes = Vec::new();
        while let Some(token) = self.lexer.next() {
            nodes.push(match token.token_type {
                TokenType::Text => TokenTree::Text(Text::new(token.at)),
                TokenType::Comment => continue,
                TokenType::Variable => parse_variable(
                    self.template,
                    token.content(self.template),
                    token.at,
                    token.at.0 + START_TAG_LEN,
                )?
                .into(),
                TokenType::Tag => self.parse_tag(token.content(self.template), token.at)?,
            })
        }
        Ok(nodes)
    }

    fn parse_tag(&mut self, tag: &'t str, at: (usize, usize)) -> Result<TokenTree, ParseError> {
        let (tag, parts) = match lex_tag(tag, at.0 + START_TAG_LEN)? {
            None => return Err(ParseError::EmptyTag { at: at.into() }),
            Some(t) => t,
        };
        match tag.content(self.template) {
            "url" => self.parse_url(at, parts),
            _ => todo!(),
        }
    }

    fn parse_url(&mut self, at: (usize, usize), parts: TagParts) -> Result<TokenTree, ParseError> {
        let mut lexer = UrlLexer::new(self.template, parts);
        let view_name = match lexer.next() {
            Some(view_token) => view_token?.parse(self.template)?,
            None => return Err(ParseError::UrlTagNoArguments { at: at.into() }),
        };

        let mut tokens = vec![];
        for token in lexer {
            tokens.push(token?);
        }
        let mut rev = tokens.iter().rev();
        let variable = match (rev.next(), rev.next()) {
            (
                Some(UrlToken {
                    at: last,
                    token_type: UrlTokenType::Variable,
                    ..
                }),
                Some(UrlToken {
                    at: prev,
                    token_type: UrlTokenType::Variable,
                    ..
                }),
            ) => {
                let prev = &self.template[prev.0..prev.0 + prev.1];
                if prev == "as" {
                    Some(self.template[last.0..last.0 + last.1].to_string())
                } else {
                    None
                }
            }
            _ => None,
        };
        if variable.is_some() {
            tokens.truncate(tokens.len() - 2)
        }
        let mut args = vec![];
        let mut kwargs = vec![];
        for token in tokens {
            let element = token.parse(self.template)?;
            match token.kwarg {
                None => args.push(element),
                Some((start, len)) => {
                    let kwarg = self.template[start..start + len].to_string();
                    kwargs.push((kwarg, element));
                }
            }
        }
        if !args.is_empty() && !kwargs.is_empty() {
            return Err(ParseError::MixedArgsKwargs { at: at.into() });
        }
        let url = Url {
            view_name,
            args,
            kwargs,
            variable,
        };
        Ok(TokenTree::Tag(Tag::Url(url)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::lex::common::LexerError;

    #[test]
    fn test_empty_template() {
        let template = "";
        let mut parser = Parser::new(template);
        let nodes = parser.parse().unwrap();
        assert_eq!(nodes, vec![]);
    }

    #[test]
    fn test_text() {
        let template = "Some text";
        let mut parser = Parser::new(template);
        let nodes = parser.parse().unwrap();
        let text = Text::new((0, template.len()));
        assert_eq!(nodes, vec![TokenTree::Text(text)]);
        assert_eq!(text.content(template), template);
    }

    #[test]
    fn test_comment() {
        let template = "{# A commment #}";
        let mut parser = Parser::new(template);
        let nodes = parser.parse().unwrap();
        assert_eq!(nodes, vec![]);
    }

    #[test]
    fn test_empty_variable() {
        let template = "{{ }}";
        let mut parser = Parser::new(template);
        let error = parser.parse().unwrap_err();
        assert_eq!(error, ParseError::EmptyVariable { at: (0, 5).into() });
    }

    #[test]
    fn test_variable() {
        let template = "{{ foo }}";
        let mut parser = Parser::new(template);
        let nodes = parser.parse().unwrap();
        let variable = Variable { at: (3, 3) };
        assert_eq!(nodes, vec![TokenTree::Variable(variable)]);
        assert_eq!(variable.parts(template).collect::<Vec<_>>(), vec!["foo"]);
    }

    #[test]
    fn test_variable_attribute() {
        let template = "{{ foo.bar.baz }}";
        let mut parser = Parser::new(template);
        let nodes = parser.parse().unwrap();
        let variable = Variable { at: (3, 11) };
        assert_eq!(nodes, vec![TokenTree::Variable(variable)]);
        assert_eq!(
            variable.parts(template).collect::<Vec<_>>(),
            vec!["foo", "bar", "baz"]
        );
    }

    #[test]
    fn test_filter() {
        let template = "{{ foo|bar }}";
        let mut parser = Parser::new(template);
        let nodes = parser.parse().unwrap();

        let foo = Variable { at: (3, 3) };
        let bar = TokenTree::Filter(Box::new(Filter {
            at: (7, 3),
            left: TagElement::Variable(foo),
            filter: FilterType::External(None),
        }));
        assert_eq!(nodes, vec![bar]);
        assert_eq!(foo.parts(template).collect::<Vec<_>>(), vec!["foo"]);
    }

    #[test]
    fn test_filter_multiple() {
        let template = "{{ foo|bar|baz }}";
        let mut parser = Parser::new(template);
        let nodes = parser.parse().unwrap();

        let foo = TagElement::Variable(Variable { at: (3, 3) });
        let bar = TagElement::Filter(Box::new(Filter {
            at: (7, 3),
            left: foo,
            filter: FilterType::External(None),
        }));
        let baz = TokenTree::Filter(Box::new(Filter {
            at: (11, 3),
            left: bar,
            filter: FilterType::External(None),
        }));
        assert_eq!(nodes, vec![baz]);
    }

    #[test]
    fn test_filter_argument() {
        let template = "{{ foo|bar:baz }}";
        let mut parser = Parser::new(template);
        let nodes = parser.parse().unwrap();

        let foo = TagElement::Variable(Variable { at: (3, 3) });
        let baz = Variable { at: (11, 3) };
        let bar = TokenTree::Filter(Box::new(Filter {
            at: (7, 3),
            left: foo,
            filter: FilterType::External(Some(Argument {
                at: (11, 3),
                argument_type: ArgumentType::Variable(baz),
            })),
        }));
        assert_eq!(nodes, vec![bar]);
        assert_eq!(baz.parts(template).collect::<Vec<_>>(), vec!["baz"]);
    }

    #[test]
    fn test_filter_argument_text() {
        let template = "{{ foo|bar:'baz' }}";
        let mut parser = Parser::new(template);
        let nodes = parser.parse().unwrap();

        let foo = TagElement::Variable(Variable { at: (3, 3) });
        let baz = Text::new((12, 3));
        let bar = TokenTree::Filter(Box::new(Filter {
            at: (7, 3),
            left: foo,
            filter: FilterType::External(Some(Argument {
                at: (11, 5),
                argument_type: ArgumentType::Text(baz),
            })),
        }));
        assert_eq!(nodes, vec![bar]);
        assert_eq!(baz.content(template), "baz");
    }

    #[test]
    fn test_filter_argument_translated_text() {
        let template = "{{ foo|bar:_('baz') }}";
        let mut parser = Parser::new(template);
        let nodes = parser.parse().unwrap();

        let foo = TagElement::Variable(Variable { at: (3, 3) });
        let baz = Text::new((14, 3));
        let bar = TokenTree::Filter(Box::new(Filter {
            at: (7, 3),
            left: foo,
            filter: FilterType::External(Some(Argument {
                at: (11, 8),
                argument_type: ArgumentType::TranslatedText(baz),
            })),
        }));
        assert_eq!(nodes, vec![bar]);
        assert_eq!(baz.content(template), "baz");
    }

    #[test]
    fn test_filter_argument_float() {
        let template = "{{ foo|bar:5.2e3 }}";
        let mut parser = Parser::new(template);
        let nodes = parser.parse().unwrap();

        let foo = TagElement::Variable(Variable { at: (3, 3) });
        let num = Argument {
            at: (11, 5),
            argument_type: ArgumentType::Float(5.2e3),
        };
        let bar = TokenTree::Filter(Box::new(Filter {
            at: (7, 3),
            left: foo,
            filter: FilterType::External(Some(num)),
        }));
        assert_eq!(nodes, vec![bar]);
    }

    #[test]
    fn test_filter_argument_int() {
        let template = "{{ foo|bar:99 }}";
        let mut parser = Parser::new(template);
        let nodes = parser.parse().unwrap();

        let foo = TagElement::Variable(Variable { at: (3, 3) });
        let num = Argument {
            at: (11, 2),
            argument_type: ArgumentType::Int(99.into()),
        };
        let bar = TokenTree::Filter(Box::new(Filter {
            at: (7, 3),
            left: foo,
            filter: FilterType::External(Some(num)),
        }));
        assert_eq!(nodes, vec![bar]);
    }

    #[test]
    fn test_filter_argument_bigint() {
        let template = "{{ foo|bar:99999999999999999 }}";
        let mut parser = Parser::new(template);
        let nodes = parser.parse().unwrap();

        let foo = TagElement::Variable(Variable { at: (3, 3) });
        let num = Argument {
            at: (11, 17),
            argument_type: ArgumentType::Int("99999999999999999".parse::<BigInt>().unwrap()),
        };
        let bar = TokenTree::Filter(Box::new(Filter {
            at: (7, 3),
            left: foo,
            filter: FilterType::External(Some(num)),
        }));
        assert_eq!(nodes, vec![bar]);
    }

    #[test]
    fn test_filter_argument_invalid_number() {
        let template = "{{ foo|bar:9.9.9 }}";
        let mut parser = Parser::new(template);
        let error = parser.parse().unwrap_err();
        assert_eq!(error, ParseError::InvalidNumber { at: (11, 5).into() });
    }

    #[test]
    fn test_filter_default() {
        let template = "{{ foo|default:baz }}";
        let mut parser = Parser::new(template);
        let nodes = parser.parse().unwrap();

        let foo = TagElement::Variable(Variable { at: (3, 3) });
        let baz = Variable { at: (15, 3) };
        let bar = TokenTree::Filter(Box::new(Filter {
            at: (7, 7),
            left: foo,
            filter: FilterType::Default(Argument {
                at: (15, 3),
                argument_type: ArgumentType::Variable(baz),
            }),
        }));
        assert_eq!(nodes, vec![bar]);
        assert_eq!(baz.parts(template).collect::<Vec<_>>(), vec!["baz"]);
    }

    #[test]
    fn test_filter_default_missing_argument() {
        let template = "{{ foo|default|baz }}";
        let mut parser = Parser::new(template);
        let error = parser.parse().unwrap_err();
        assert_eq!(error, ParseError::MissingArgument { at: (7, 7).into() });
    }

    #[test]
    fn test_filter_lower_unexpected_argument() {
        let template = "{{ foo|lower:baz }}";
        let mut parser = Parser::new(template);
        let error = parser.parse().unwrap_err();
        assert_eq!(error, ParseError::UnexpectedArgument { at: (13, 3).into() });
    }

    #[test]
    fn test_variable_lexer_error() {
        let template = "{{ _foo }}";
        let mut parser = Parser::new(template);
        let error = parser.parse().unwrap_err();
        assert_eq!(
            error,
            ParseError::VariableError(LexerError::InvalidVariableName { at: (3, 4).into() }.into())
        );
    }

    #[test]
    fn test_parse_empty_tag() {
        let template = "{%  %}";
        let mut parser = Parser::new(template);
        let error = parser.parse().unwrap_err();
        assert_eq!(error, ParseError::EmptyTag { at: (0, 6).into() });
    }

    #[test]
    fn test_block_error() {
        let template = "{% url'foo' %}";
        let mut parser = Parser::new(template);
        let error = parser.parse().unwrap_err();
        assert_eq!(
            error,
            ParseError::BlockError(TagLexerError::InvalidTagName { at: (3, 8).into() })
        );
    }

    #[test]
    fn test_parse_url_tag() {
        let template = "{% url 'some-url-name' %}";
        let mut parser = Parser::new(template);
        let nodes = parser.parse().unwrap();

        let url = TokenTree::Tag(Tag::Url(Url {
            view_name: TagElement::Text(Text { at: (8, 13) }),
            args: vec![],
            kwargs: vec![],
            variable: None,
        }));

        assert_eq!(nodes, vec![url]);
    }

    #[test]
    fn test_parse_url_tag_view_name_translated() {
        let template = "{% url _('some-url-name') %}";
        let mut parser = Parser::new(template);
        let nodes = parser.parse().unwrap();

        let url = TokenTree::Tag(Tag::Url(Url {
            view_name: TagElement::TranslatedText(Text { at: (10, 13) }),
            args: vec![],
            kwargs: vec![],
            variable: None,
        }));

        assert_eq!(nodes, vec![url]);
    }

    #[test]
    fn test_parse_url_tag_view_name_variable() {
        let template = "{% url some_view_name %}";
        let mut parser = Parser::new(template);
        let nodes = parser.parse().unwrap();

        let url = TokenTree::Tag(Tag::Url(Url {
            view_name: TagElement::Variable(Variable { at: (7, 14) }),
            args: vec![],
            kwargs: vec![],
            variable: None,
        }));

        assert_eq!(nodes, vec![url]);
    }

    #[test]
    fn test_parse_url_tag_view_name_filter() {
        let template = "{% url some_view_name|default:'home' %}";
        let mut parser = Parser::new(template);
        let nodes = parser.parse().unwrap();

        let some_view_name = TagElement::Variable(Variable { at: (7, 14) });
        let home = Text { at: (31, 4) };
        let default = Box::new(Filter {
            at: (22, 7),
            left: some_view_name,
            filter: FilterType::Default(Argument {
                at: (30, 6),
                argument_type: ArgumentType::Text(home),
            }),
        });
        let url = TokenTree::Tag(Tag::Url(Url {
            view_name: TagElement::Filter(default),
            args: vec![],
            kwargs: vec![],
            variable: None,
        }));

        assert_eq!(nodes, vec![url]);
    }

    #[test]
    fn test_parse_url_no_arguments() {
        let template = "{% url %}";
        let mut parser = Parser::new(template);
        let error = parser.parse().unwrap_err();
        assert_eq!(error, ParseError::UrlTagNoArguments { at: (0, 9).into() });
    }

    #[test]
    fn test_parse_url_view_name_integer() {
        let template = "{% url 64 %}";
        let mut parser = Parser::new(template);
        let nodes = parser.parse().unwrap();

        let url = TokenTree::Tag(Tag::Url(Url {
            view_name: TagElement::Int(64.into()),
            args: vec![],
            kwargs: vec![],
            variable: None,
        }));

        assert_eq!(nodes, vec![url]);
    }

    #[test]
    fn test_parse_url_tag_arguments() {
        let template = "{% url some_view_name 'foo' bar|default:'home' 64 5.7 _(\"spam\") %}";
        let mut parser = Parser::new(template);
        let nodes = parser.parse().unwrap();

        let url = TokenTree::Tag(Tag::Url(Url {
            view_name: TagElement::Variable(Variable { at: (7, 14) }),
            args: vec![
                TagElement::Text(Text { at: (23, 3) }),
                TagElement::Filter(Box::new(Filter {
                    at: (32, 7),
                    left: TagElement::Variable(Variable { at: (28, 3) }),
                    filter: FilterType::Default(Argument {
                        at: (40, 6),
                        argument_type: ArgumentType::Text(Text { at: (41, 4) }),
                    }),
                })),
                TagElement::Int(64.into()),
                TagElement::Float(5.7),
                TagElement::TranslatedText(Text { at: (57, 4) }),
            ],
            kwargs: vec![],
            variable: None,
        }));

        assert_eq!(nodes, vec![url]);
    }

    #[test]
    fn test_parse_url_tag_kwargs() {
        let template = "{% url some_view_name foo='foo' extra=-64 %}";
        let mut parser = Parser::new(template);
        let nodes = parser.parse().unwrap();

        let url = TokenTree::Tag(Tag::Url(Url {
            view_name: TagElement::Variable(Variable { at: (7, 14) }),
            args: vec![],
            kwargs: vec![
                ("foo".to_string(), TagElement::Text(Text { at: (27, 3) })),
                ("extra".to_string(), TagElement::Int((-64).into())),
            ],
            variable: None,
        }));

        assert_eq!(nodes, vec![url]);
    }

    #[test]
    fn test_parse_url_tag_arguments_as_variable() {
        let template = "{% url some_view_name 'foo' as some_url %}";
        let mut parser = Parser::new(template);
        let nodes = parser.parse().unwrap();

        let url = TokenTree::Tag(Tag::Url(Url {
            view_name: TagElement::Variable(Variable { at: (7, 14) }),
            args: vec![TagElement::Text(Text { at: (23, 3) })],
            kwargs: vec![],
            variable: Some("some_url".to_string()),
        }));

        assert_eq!(nodes, vec![url]);
    }

    #[test]
    fn test_parse_url_tag_kwargs_as_variable() {
        let template = "{% url some_view_name foo='foo' as some_url %}";
        let mut parser = Parser::new(template);
        let nodes = parser.parse().unwrap();

        let url = TokenTree::Tag(Tag::Url(Url {
            view_name: TagElement::Variable(Variable { at: (7, 14) }),
            args: vec![],
            kwargs: vec![("foo".to_string(), TagElement::Text(Text { at: (27, 3) }))],
            variable: Some("some_url".to_string()),
        }));

        assert_eq!(nodes, vec![url]);
    }

    #[test]
    fn test_parse_url_tag_arguments_last_variables() {
        let template = "{% url some_view_name 'foo' arg arg2 %}";
        let mut parser = Parser::new(template);
        let nodes = parser.parse().unwrap();

        let url = TokenTree::Tag(Tag::Url(Url {
            view_name: TagElement::Variable(Variable { at: (7, 14) }),
            args: vec![
                TagElement::Text(Text { at: (23, 3) }),
                TagElement::Variable(Variable { at: (28, 3) }),
                TagElement::Variable(Variable { at: (32, 4) }),
            ],
            kwargs: vec![],
            variable: None,
        }));

        assert_eq!(nodes, vec![url]);
    }

    #[test]
    fn test_parse_url_tag_mixed_args_kwargs() {
        let template = "{% url some_view_name 'foo' arg name=arg2 %}";
        let mut parser = Parser::new(template);
        let error = parser.parse().unwrap_err();
        assert_eq!(
            error,
            ParseError::MixedArgsKwargs {
                at: (0, template.len()).into()
            }
        );
    }

    #[test]
    fn test_parse_url_tag_invalid_number() {
        let template = "{% url foo 9.9.9 %}";
        let mut parser = Parser::new(template);
        let error = parser.parse().unwrap_err();
        assert_eq!(error, ParseError::InvalidNumber { at: (11, 5).into() });
    }
}
