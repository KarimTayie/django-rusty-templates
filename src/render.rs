use std::borrow::Cow;
use std::collections::HashMap;

use num_bigint::BigInt;
use pyo3::prelude::*;

use crate::parse::{Argument, ArgumentType, Filter, FilterType, TokenTree, Variable};

pub enum Content<'t, 'py> {
    Py(Bound<'py, PyAny>),
    String(Cow<'t, str>),
    Float(f64),
    Int(BigInt),
}

impl<'t> Content<'t, '_> {
    fn render(self) -> PyResult<Cow<'t, str>> {
        let content = match self {
            Self::Py(content) => content.str()?.extract::<String>()?,
            Self::String(content) => return Ok(content),
            Self::Float(content) => content.to_string(),
            Self::Int(content) => content.to_string(),
        };
        Ok(Cow::Owned(content))
    }
}

pub trait Render {
    fn resolve<'t, 'py>(
        &self,
        py: Python<'py>,
        template: &'t str,
        context: &HashMap<String, Bound<'py, PyAny>>,
    ) -> PyResult<Option<Content<'t, 'py>>>;

    fn render<'t, 'py>(
        &self,
        py: Python<'py>,
        template: &'t str,
        context: &HashMap<String, Bound<'py, PyAny>>,
    ) -> PyResult<Cow<'t, str>> {
        let content = match self.resolve(py, template, context) {
            Ok(Some(content)) => return content.render(),
            Ok(None) => "".to_string(),
            Err(_) => "".to_string(),
        };
        Ok(Cow::Owned(content))
    }
}

impl Render for Variable {
    fn resolve<'t, 'py>(
        &self,
        _py: Python<'py>,
        template: &'t str,
        context: &HashMap<String, Bound<'py, PyAny>>,
    ) -> PyResult<Option<Content<'t, 'py>>> {
        let mut parts = self.parts(template);
        let first = parts.next().expect("Variable names cannot be empty");
        let mut variable = match context.get(first) {
            Some(variable) => variable.clone(),
            None => return Ok(None),
        };
        for part in parts {
            variable = match variable.get_item(part) {
                Ok(variable) => variable,
                Err(_) => match variable.getattr(part) {
                    Ok(variable) => variable,
                    Err(e) => {
                        let int = match part.parse::<usize>() {
                            Ok(int) => int,
                            Err(_) => return Err(e),
                        };
                        match variable.get_item(int) {
                            Ok(variable) => variable,
                            Err(_) => todo!(),
                        }
                    }
                },
            }
        }
        Ok(Some(Content::Py(variable)))
    }
}

impl Render for Filter {
    fn resolve<'t, 'py>(
        &self,
        py: Python<'py>,
        template: &'t str,
        context: &HashMap<String, Bound<'py, PyAny>>,
    ) -> PyResult<Option<Content<'t, 'py>>> {
        let left = self.left.resolve(py, template, context)?;
        Ok(match &self.filter {
            FilterType::Default(right) => match left {
                Some(left) => Some(left),
                None => right.resolve(py, template, context)?,
            },
            FilterType::External(_filter) => todo!(),
            FilterType::Lower => match left {
                Some(content) => Some(Content::String(Cow::Owned(content.render()?.to_lowercase()))),
                None => Some(Content::String(Cow::Borrowed(""))),
            }
        })
    }
}

impl Render for TokenTree {
    fn resolve<'t, 'py>(
        &self,
        py: Python<'py>,
        template: &'t str,
        context: &HashMap<String, Bound<'py, PyAny>>,
    ) -> PyResult<Option<Content<'t, 'py>>> {
        match self {
            TokenTree::Text(text) => {
                Ok(Some(Content::String(Cow::Borrowed(text.content(template)))))
            }
            TokenTree::TranslatedText(_text) => todo!(),
            TokenTree::Tag(_tag) => todo!(),
            TokenTree::Variable(variable) => variable.resolve(py, template, context),
            TokenTree::Filter(filter) => filter.resolve(py, template, context),
        }
    }
}

impl Render for Argument {
    fn resolve<'t, 'py>(
        &self,
        py: Python<'py>,
        template: &'t str,
        context: &HashMap<String, Bound<'py, PyAny>>,
    ) -> PyResult<Option<Content<'t, 'py>>> {
        Ok(Some(match &self.argument_type {
            ArgumentType::Text(text) => {
                Content::String(Cow::Borrowed(text.content(template)))
            }
            ArgumentType::TranslatedText(_text) => todo!(),
            ArgumentType::Variable(variable) => return variable.resolve(py, template, context),
            ArgumentType::Float(number) => Content::Float(*number),
            ArgumentType::Int(number) => Content::Int(number.clone()),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use pyo3::types::{PyDict, PyList, PyString};

    use crate::parse::Text;

    #[test]
    fn test_render_variable() {
        pyo3::prepare_freethreaded_python();

        Python::with_gil(|py| {
            let name = PyString::new(py, "Lily").into_any();
            let context = HashMap::from([("name".to_string(), name)]);
            let template = "{{ name }}";
            let variable = Variable::new((3, 4));

            let rendered = variable.render(py, template, &context).unwrap();
            assert_eq!(rendered, "Lily");
        })
    }

    #[test]
    fn test_render_dict_lookup() {
        pyo3::prepare_freethreaded_python();

        Python::with_gil(|py| {
            let data = PyDict::new(py);
            let name = PyString::new(py, "Lily");
            data.set_item("name", name).unwrap();
            let context = HashMap::from([("data".to_string(), data.into_any())]);
            let template = "{{ data.name }}";
            let variable = Variable::new((3, 9));

            let rendered = variable.render(py, template, &context).unwrap();
            assert_eq!(rendered, "Lily");
        })
    }

    #[test]
    fn test_render_list_lookup() {
        pyo3::prepare_freethreaded_python();

        Python::with_gil(|py| {
            let name = PyString::new(py, "Lily");
            let names = PyList::new(py, [name]).unwrap();
            let context = HashMap::from([("names".to_string(), names.into_any())]);
            let template = "{{ names.0 }}";
            let variable = Variable::new((3, 7));

            let rendered = variable.render(py, template, &context).unwrap();
            assert_eq!(rendered, "Lily");
        })
    }

    #[test]
    fn test_render_attribute_lookup() {
        pyo3::prepare_freethreaded_python();

        Python::with_gil(|py| {
            let locals = PyDict::new(py);
            py.run(
                c"
class User:
    def __init__(self, name):
        self.name = name

user = User('Lily')
",
                None,
                Some(&locals),
            ).unwrap();

            let context = locals.extract().unwrap();
            let template = "{{ user.name }}";
            let variable = Variable::new((3, 9));

            let rendered = variable.render(py, template, &context).unwrap();
            assert_eq!(rendered, "Lily");
        })
    }

    #[test]
    fn test_render_filter() {
        pyo3::prepare_freethreaded_python();

        Python::with_gil(|py| {
            let name = PyString::new(py, "Lily").into_any();
            let context = HashMap::from([("name".to_string(), name)]);
            let template = "{{ name|default:'Bryony' }}";
            let variable = Variable::new((3, 4));
            let filter = Filter::new(
                template,
                (8, 7),
                TokenTree::Variable(variable),
                Some(Argument { at: (16, 8), argument_type: ArgumentType::Text(Text::new((17, 6)))}),
            ).unwrap();

            let rendered = filter.render(py, template, &context).unwrap();
            assert_eq!(rendered, "Lily");
        })
    }

    #[test]
    fn test_render_filter_default() {
        pyo3::prepare_freethreaded_python();

        Python::with_gil(|py| {
            let context = HashMap::new();
            let template = "{{ name|default:'Bryony' }}";
            let variable = Variable::new((3, 4));
            let filter = Filter::new(
                template,
                (8, 7),
                TokenTree::Variable(variable),
                Some(Argument{ at: (16, 8), argument_type: ArgumentType::Text(Text::new((17, 6)))}),
            ).unwrap();

            let rendered = filter.render(py, template, &context).unwrap();
            assert_eq!(rendered, "Bryony");
        })
    }

    #[test]
    fn test_render_filter_default_integer() {
        pyo3::prepare_freethreaded_python();

        Python::with_gil(|py| {
            let context = HashMap::new();
            let template = "{{ count|default:12}}";
            let variable = Variable::new((3, 5));
            let filter = Filter::new(
                template,
                (9, 7),
                TokenTree::Variable(variable),
                Some(Argument { at: (17, 2), argument_type: ArgumentType::Int(12.into())}),
            ).unwrap();

            let rendered = filter.render(py, template, &context).unwrap();
            assert_eq!(rendered, "12");
        })
    }

    #[test]
    fn test_render_filter_default_float() {
        pyo3::prepare_freethreaded_python();

        Python::with_gil(|py| {
            let context = HashMap::new();
            let template = "{{ count|default:3.5}}";
            let variable = Variable::new((3, 5));
            let filter = Filter::new(
                template,
                (9, 7),
                TokenTree::Variable(variable),
                Some(Argument{ at: (17, 3), argument_type: ArgumentType::Float(3.5)}),
            ).unwrap();

            let rendered = filter.render(py, template, &context).unwrap();
            assert_eq!(rendered, "3.5");
        })
    }

    #[test]
    fn test_render_filter_default_variable() {
        pyo3::prepare_freethreaded_python();

        Python::with_gil(|py| {
            let me = PyString::new(py, "Lily").into_any();
            let context = HashMap::from([("me".to_string(), me)]);
            let template = "{{ name|default:me}}";
            let variable = Variable::new((3, 4));
            let filter = Filter::new(
                template,
                (8, 7),
                TokenTree::Variable(variable),
                Some(Argument{ at: (16, 2), argument_type: ArgumentType::Variable(Variable::new((16, 2)))}),
            ).unwrap();

            let rendered = filter.render(py, template, &context).unwrap();
            assert_eq!(rendered, "Lily");
        })
    }

    #[test]
    fn test_render_filter_lower() {
        pyo3::prepare_freethreaded_python();

        Python::with_gil(|py| {
            let name = PyString::new(py, "Lily").into_any();
            let context = HashMap::from([("name".to_string(), name)]);
            let template = "{{ name|lower }}";
            let variable = Variable::new((3, 4));
            let filter = Filter::new(
                template,
                (8, 5),
                TokenTree::Variable(variable),
                None,
            ).unwrap();

            let rendered = filter.render(py, template, &context).unwrap();
            assert_eq!(rendered, "lily");
        })
    }

    #[test]
    fn test_render_filter_lower_missing_left() {
        pyo3::prepare_freethreaded_python();

        Python::with_gil(|py| {
            let context = HashMap::new();
            let template = "{{ name|lower }}";
            let variable = Variable::new((3, 4));
            let filter = Filter::new(
                template,
                (8, 5),
                TokenTree::Variable(variable),
                None,
            ).unwrap();

            let rendered = filter.render(py, template, &context).unwrap();
            assert_eq!(rendered, "");
        })
    }

    #[test]
    fn test_render_chained_filters() {
        pyo3::prepare_freethreaded_python();

        Python::with_gil(|py| {
            let context = HashMap::new();
            let template = "{{ name|default:'Bryony'|lower }}";
            let variable = Variable::new((3, 4));
            let default = Filter::new(
                template,
                (8, 7),
                TokenTree::Variable(variable),
                Some(Argument { at: (16, 8), argument_type: ArgumentType::Text(Text::new((17, 6)))}),
            ).unwrap();
            let lower = Filter::new(
                template,
                (25, 5),
                TokenTree::Filter(Box::new(default)),
                None,
            ).unwrap();

            let rendered = lower.render(py, template, &context).unwrap();
            assert_eq!(rendered, "bryony");
        })
    }
}
