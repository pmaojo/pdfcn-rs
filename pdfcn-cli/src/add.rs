//! "Copy-paste mode" (FR-2): drops a preset component's HAML usage snippet
//! into the developer's own `./templates/components/` tree so it can be
//! edited freely, the same philosophy shadcn/ui uses for React components.

fn snippet(component: &str) -> Option<&'static str> {
    match component {
        "Card" => Some(
            "%Card(title=\"{{ title }}\")\n  %p {{ body }}\n",
        ),
        "Badge" => Some("%Badge(variant=\"default\" label=\"{{ label }}\")\n"),
        "InvoiceTable" => Some(
            "%InvoiceTable(rows={{ rows }} columns={{ columns }})\n",
        ),
        "SignatureBlock" => Some("%SignatureBlock(name=\"{{ signer_name }}\" label=\"Signature\")\n"),
        "Grid" => Some("%Grid(cols=\"2\")\n  %div Left\n  %div Right\n"),
        _ => None,
    }
}

pub fn run(component: &str) -> anyhow::Result<()> {
    let content = snippet(component).ok_or_else(|| {
        anyhow::anyhow!(
            "unknown component '{component}'. Available: Card, Badge, InvoiceTable, SignatureBlock, Grid"
        )
    })?;

    let dir = std::path::Path::new("templates/components");
    std::fs::create_dir_all(dir)?;
    let path = dir.join(format!("{component}.haml"));
    if path.exists() {
        anyhow::bail!("{path:?} already exists, refusing to overwrite");
    }
    std::fs::write(&path, content)?;
    println!("Added {path:?} — edit it freely, it's yours now.");
    Ok(())
}
