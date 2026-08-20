//! `%Avatar`: shadcn's Avatar — a circular image, or an initials fallback
//! (`AvatarFallback`) when no image source is given.

use maud::{html, Markup};
use pdfcn_template::ResolvedAttr;

use crate::{attr, attr_or};

fn size_classes(size: &str) -> &'static str {
    match size {
        "sm" => "h-8 w-8",
        "lg" => "h-12 w-12",
        _ => "h-10 w-10",
    }
}

fn initials(name: &str) -> String {
    name.split_whitespace()
        .filter_map(|word| word.chars().next())
        .take(2)
        .collect::<String>()
        .to_uppercase()
}

pub fn avatar(attrs: &[ResolvedAttr]) -> Markup {
    let size = attr_or(attrs, "size", "md");
    let size_class = size_classes(size);
    match attr(attrs, "src") {
        Some(src) => {
            let alt = attr_or(attrs, "alt", "");
            html! {
                img class={ "avatar rounded-full object-cover " (size_class) } src=(src) alt=(alt);
            }
        }
        None => {
            let name = attr_or(attrs, "name", "");
            html! {
                span class={ "avatar avatar-fallback inline-flex items-center justify-center rounded-full bg-muted text-muted-foreground font-medium " (size_class) } {
                    (initials(name))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(name: &str, value: &str) -> ResolvedAttr {
        ResolvedAttr {
            name: name.into(),
            value: value.into(),
        }
    }

    #[test]
    fn image_avatar_is_a_circular_sized_image() {
        let out = avatar(&[a("src", "https://example.com/a.png"), a("alt", "Ada")]).into_string();
        assert!(out.contains("<img"));
        assert!(out.contains("rounded-full"));
        assert!(out.contains("h-10 w-10"));
        assert!(out.contains("https://example.com/a.png"));
    }

    #[test]
    fn falls_back_to_initials_without_a_src() {
        let out = avatar(&[a("name", "Ada Lovelace")]).into_string();
        assert!(out.contains("AL"));
        assert!(out.contains("avatar-fallback"));
    }
}
