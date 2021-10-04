use std::collections::BTreeMap;
use std::sync::Arc;
use tldextract::*;
use vrl::prelude::*;

#[derive(Clone, Copy, Debug)]
pub struct ParseHost;

impl Function for ParseHost {
    fn identifier(&self) -> &'static str {
        "parse_host"
    }

    fn summary(&self) -> &'static str {
        "parse a URL or domain name string"
    }

    fn usage(&self) -> &'static str {
        indoc! {r#"
            Parses the provided `value` into TLD, registered domain, and subdomains.

            By default includes private domains.
        "#}
    }

    fn parameters(&self) -> &'static [Parameter] {
        &[
            Parameter {
                keyword: "value",
                kind: kind::BYTES,
                required: true,
            },
            Parameter {
                keyword: "private_domains",
                kind: kind::BOOLEAN,
                required: false,
            },
            Parameter {
                keyword: "update_cache",
                kind: kind::BOOLEAN,
                required: false,
            },
        ]
    }

    fn examples(&self) -> &'static [Example] {
        &[
            Example {
                title: "parse domain name",
                source: r#"parse_host!("www.example.com")"#,
                result: Ok(indoc! {r#"
                    {
                        "domain": "example",
                        "subdomain": "www",
                        "suffix": "com"
                    }
                "#}),
            },
            Example {
                title: "parse URL",
                source: r#"parse_host!("https://www.example.com/s?q=foobar")"#,
                result: Ok(indoc! {r#"
                    {
                        "domain": "example",
                        "subdomain": "www",
                        "suffix": "com"
                    }
                "#}),
            },
            Example {
                title: "ignore private domain",
                source: r#"parse_host!("internal.us-east-1.elb.amazonaws.com", private_domains: false)"#,
                result: Ok(indoc! {r#"
                    {
                        "domain": "amazonaws",
                        "subdomain": "internal.us-east-1.elb",
                        "suffix": "com"
                    }
                "#}),
            },
        ]
    }

    fn compile(&self, state: &state::Compiler, mut arguments: ArgumentList) -> Compiled {
        let data_dir: String = state
            .get_external_context::<std::path::PathBuf>()
            .unwrap()
            .as_os_str()
            .to_os_string()
            .into_string()
            .unwrap();
        let value = arguments.required("value");

        let private_domains = arguments
            .optional("private_domains")
            .unwrap_or_else(|| expr!(true));

        let update_cache = arguments
            .optional_literal("update_cache")?
            .map(|literal| literal.to_value().try_boolean())
            .unwrap_or(Ok(false))
            .expect("update_cache should be boolean");

        let extractor = Arc::new(TldExtractor::new(TldOption {
            cache_path: Some(format!("{}/.tld_cache", data_dir)),
            update_local: update_cache,
            ..Default::default()
        }));
        let private_extractor = Arc::new(TldExtractor::new(TldOption {
            cache_path: Some(format!("{}/.private_tld_cache", data_dir)),
            update_local: update_cache,
            private_domains: true,
            ..Default::default()
        }));

        Ok(Box::new(ParseHostFn {
            value,
            extractor,
            private_domains,
            private_extractor,
        }))
    }
}

#[derive(Debug, Clone)]
struct ParseHostFn {
    value: Box<dyn Expression>,
    extractor: Arc<TldExtractor>,
    private_domains: Box<dyn Expression>,
    private_extractor: Arc<TldExtractor>,
}

impl Expression for ParseHostFn {
    fn resolve(&self, ctx: &mut Context) -> Resolved {
        let value = self.value.resolve(ctx)?;
        let string = value.try_bytes_utf8_lossy()?;

        let private_domains = self.private_domains.resolve(ctx)?.try_boolean()?;

        let ext = if private_domains {
            &self.private_extractor
        } else {
            &self.extractor
        };
        let tld = ext.extract(&string).unwrap();
        Ok(tld_to_value(tld))
    }

    fn type_def(&self, _state: &state::Compiler) -> TypeDef {
        TypeDef::new().fallible().object(type_def())
    }
}

fn tld_to_value(tld: TldResult) -> Value {
    let mut map = BTreeMap::<&str, Value>::new();

    map.insert("domain", tld.domain.into());
    map.insert("subdomain", tld.subdomain.into());
    map.insert("suffix", tld.suffix.into());

    map.into_iter()
        .map(|(k, v)| (k.to_owned(), v))
        .collect::<Value>()
}

fn type_def() -> BTreeMap<&'static str, TypeDef> {
    map! {
        "domain": Kind::Bytes | Kind::Null,
        "subdomain": Kind::Bytes | Kind::Null,
        "suffix": Kind::Bytes | Kind::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    test_function![
        parse_host => ParseHost;

        multiple_subdomains {
            args: func_args![value: value!("sub.asdf.example.com")],
            want: Ok(value!({
                domain: "example",
                subdomain: "sub.asdf",
                suffix: "com",
            })),
            tdef: TypeDef::new().fallible().object::<&'static str, TypeDef>(type_def()),
        }

        private {
            args: func_args![value: value!("internal.us-east-1.elb.amazonaws.com")],
            want: Ok(value!({
                domain: "internal",
                subdomain: null,
                suffix: "us-east-1.elb.amazonaws.com",
            })),
            tdef: TypeDef::new().fallible().object::<&'static str, TypeDef>(type_def()),
        }

        no_private {
            args: func_args![value: value!("internal.us-east-1.elb.amazonaws.com"), private_domains: false],
            want: Ok(value!({
                domain: "amazonaws",
                subdomain: "internal.us-east-1.elb",
                suffix: "com",
            })),
            tdef: TypeDef::new().fallible().object::<&'static str, TypeDef>(type_def()),
        }
    ];
}
