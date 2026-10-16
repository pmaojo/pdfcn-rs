/// Page dimensions, in millimeters, for common sizes plus a custom escape
/// hatch (FR-4: "tamaños (A4, Letter, medidas custom)").
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PageSize {
    A4,
    Letter,
    Custom { width_mm: f32, height_mm: f32 },
}

impl PageSize {
    /// Base (portrait) dimensions in millimeters.
    pub fn dimensions_mm(self) -> (f32, f32) {
        match self {
            PageSize::A4 => (210.0, 297.0),
            PageSize::Letter => (215.9, 279.4),
            PageSize::Custom {
                width_mm,
                height_mm,
            } => (width_mm, height_mm),
        }
    }

    /// A size by name, exactly as every front-end spells it: `a4`, `letter`,
    /// or `<width>x<height>` in millimeters (`100x150`). Case-sensitive; a
    /// front-end that accepts any case lowercases first.
    pub fn from_name(name: &str) -> Option<PageSize> {
        match name {
            "a4" => Some(PageSize::A4),
            "letter" => Some(PageSize::Letter),
            custom => {
                let (w, h) = custom.split_once('x')?;
                Some(PageSize::Custom {
                    width_mm: w.parse().ok()?,
                    height_mm: h.parse().ok()?,
                })
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Orientation {
    Portrait,
    Landscape,
}

impl Orientation {
    /// `portrait` or `landscape`; case-sensitive like [`PageSize::from_name`].
    pub fn from_name(name: &str) -> Option<Orientation> {
        match name {
            "portrait" => Some(Orientation::Portrait),
            "landscape" => Some(Orientation::Landscape),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageConfig {
    pub size: PageSize,
    pub orientation: Orientation,
    pub margin_mm: f32,
}

impl Default for PageConfig {
    fn default() -> Self {
        Self {
            size: PageSize::A4,
            orientation: Orientation::Portrait,
            margin_mm: 10.0,
        }
    }
}

impl PageConfig {
    /// The page a request describes with optional names, the way the HTTP API
    /// and the napi bindings read it: anything missing *or not understood*
    /// falls back to the default (A4, portrait, 10 mm) rather than failing.
    /// (The CLI is strict instead and reports a bad `--size`.)
    pub fn from_request(
        size: Option<&str>,
        orientation: Option<&str>,
        margin_mm: Option<f32>,
    ) -> Self {
        let default = PageConfig::default();
        PageConfig {
            size: size.and_then(PageSize::from_name).unwrap_or(default.size),
            orientation: match orientation.and_then(Orientation::from_name) {
                Some(Orientation::Landscape) => Orientation::Landscape,
                _ => default.orientation,
            },
            margin_mm: margin_mm.unwrap_or(default.margin_mm),
        }
    }

    /// Final (width, height) in millimeters, orientation applied.
    pub fn page_size_mm(&self) -> (f32, f32) {
        let (w, h) = self.size.dimensions_mm();
        match self.orientation {
            Orientation::Portrait => (w, h),
            Orientation::Landscape => (h, w),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The lenient reading the HTTP API and the napi bindings each had a copy
    /// of, verbatim, before [`PageConfig::from_request`].
    fn previous(
        size: Option<&str>,
        orientation: Option<&str>,
        margin_mm: Option<f32>,
    ) -> PageConfig {
        let default = PageConfig::default();
        let size = match size {
            Some("letter") => PageSize::Letter,
            Some("a4") | None => default.size,
            Some(custom) => custom
                .split_once('x')
                .and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)))
                .map(|(width_mm, height_mm)| PageSize::Custom {
                    width_mm,
                    height_mm,
                })
                .unwrap_or(default.size),
        };
        let orientation = match orientation {
            Some("landscape") => Orientation::Landscape,
            _ => default.orientation,
        };
        PageConfig {
            size,
            orientation,
            margin_mm: margin_mm.unwrap_or(default.margin_mm),
        }
    }

    #[test]
    fn from_request_reads_every_input_as_the_front_ends_did() {
        let sizes = [
            None,
            Some("a4"),
            Some("letter"),
            Some("A4"),
            Some("Letter"),
            Some("100x150"),
            Some("100X150"),
            Some("10xabc"),
            Some("x"),
            Some(""),
            Some("1.5x2.5"),
            Some("tabloid"),
        ];
        let orientations = [
            None,
            Some("landscape"),
            Some("portrait"),
            Some("Landscape"),
            Some("sideways"),
        ];
        for size in sizes {
            for orientation in orientations {
                for margin in [None, Some(0.0), Some(25.5)] {
                    assert_eq!(
                        PageConfig::from_request(size, orientation, margin),
                        previous(size, orientation, margin),
                        "{size:?} {orientation:?} {margin:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn names_parse_exactly_and_nothing_else() {
        assert_eq!(PageSize::from_name("a4"), Some(PageSize::A4));
        assert_eq!(PageSize::from_name("letter"), Some(PageSize::Letter));
        assert_eq!(
            PageSize::from_name("100x150"),
            Some(PageSize::Custom {
                width_mm: 100.0,
                height_mm: 150.0
            })
        );
        for junk in ["A4", "x", "10xabc", "", "tabloid"] {
            assert_eq!(PageSize::from_name(junk), None, "{junk}");
        }
        assert_eq!(
            Orientation::from_name("landscape"),
            Some(Orientation::Landscape)
        );
        assert_eq!(Orientation::from_name("Landscape"), None);
    }
}
