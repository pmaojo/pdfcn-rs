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
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Orientation {
    Portrait,
    Landscape,
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
    /// Final (width, height) in millimeters, orientation applied.
    pub fn page_size_mm(&self) -> (f32, f32) {
        let (w, h) = self.size.dimensions_mm();
        match self.orientation {
            Orientation::Portrait => (w, h),
            Orientation::Landscape => (h, w),
        }
    }
}
