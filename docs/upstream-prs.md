# Upstream pull requests

The engine fixes pdfcn-rs depends on live in forks (`pmaojo/printpdf`,
`pmaojo/azul`) and are meant to go upstream (`fschutt/printpdf`,
`fschutt/azul`). Each is a self-contained branch with its own test. None has
been opened yet. `Cargo.toml` pins the `pdfcn` integration branch of each fork
by commit; drop an entry from `[patch.crates-io]` once its fix is released.

## fschutt/printpdf

| Branch | Change | Test |
|---|---|---|
| `fix/box-shadow` | Paint outset `box-shadow` in `html/bridge.rs`: azul emits `DisplayListItem::BoxShadow` and the bridge dropped it. Blur is a stack of translucent rects. | `tests/html_box_shadow.rs` |
| `fix/deterministic-resource-ids` | Name synthesized ExtGState/shading resources from their content instead of at random, so two renders of one document are byte-identical (and equal states are recorded once). | `tests/html_deterministic_ids.rs` |
| `fix/text-shadow` | Paint `text-shadow` (the bridge ignored `PushTextShadow`/`PopTextShadow`). | `tests/html_text_shadow.rs` |
| `fix/transform` | Apply CSS `transform` from `PushReferenceFrame` as a PDF `cm` (conjugated by the layout -> PDF mapping). | `reference_frame_tests` (3, point-by-point) |
| `fix/image-placeholders` | `XmlRenderOptions::image_placeholders` (id -> px size): lay an `<img>` out by size alone, with a 1-px stand-in XObject that declares that size (same `cm`). For callers that write the image stream after saving (JPEG passthrough, SVG Forms); see docs/spikes/006. | `tests/html_image_placeholders.rs` |
| `fix/tracked-boxes` | `XmlRenderOptions::track_class_prefix` + `add_xml_to_document_tracked`: report each hit-test area (text runs, interactive border boxes) of elements carrying a class prefix, per page, in pt -- found in the same pass as the pages, so callers needing positions (links, a TOC) skip a second layout; see docs/spikes/007. | `tests/html_tracked_boxes.rs` |
| `fix/optional-hyphenation` | `html` no longer implies `text_layout_hyphenation` (~3 MB of `embed_all` dictionaries); the latter joins `default`, so default users see no change. | `tests/html_hyphenation.rs` (with and without the feature) |
| `fix/img-intrinsic-size` | Pass azul an `ImageCache` naming each embedded image's decoded size (layout only sees a tagged 0x0 placeholder for `<img>`). Needed by azul's object-fit. | `bridge::tests::image_cache_names_each_images_decoded_size` |

Note for the maintainer: an earlier attempt gave `<img>` `width`/`height`
attributes from the decoded size. That changes layout (the intrinsic width
inflates percentage-width containers), so it was reverted in favor of the cache.

## fschutt/azul

| Branch | Change | Test |
|---|---|---|
| `fix/paged-opacity` | `PushOpacity`/`PopOpacity` become structure markers in the paginator (`is_push_marker`, `is_pop_marker`, `matching_pop`, `rebase_marker`), so opacity survives page splitting in paged output. | `paginate_keeps_opacity_markers_balanced_on_every_page` (fails without the fix), `paginate_omits_opacity_markers_from_pages_without_their_content` |
| `fix/feature-gating` | `azul-layout` on `master` did not build with `default-features = false` (`rasterize_svg_clip_to_r8` uses `agg_rust`; `tile_mask` is `cpurender`-only but its caller was ungated). printpdf builds it that way. | `cargo check -p azul-layout --no-default-features` |
| `fix/dense-run-y` | `DenseText::to_unified_items`/`positioned_cluster` rebuilt y from the line baseline, dropping a run shifted by `vertical-align`; use the run's own y. | `dense_expansion_keeps_a_vertically_shifted_run_where_it_was_solved` (fails without the fix) |
| `fix/pdf-transform` | CSS `transform` as reference frames in paged export (no `GpuValueCache`); frames are balanced markers in the paginator. | `paginate_keeps_reference_frames_balanced_with_an_exact_origin` |
| `fix/box-shadow-once` | Paint a box-shadow once, not once per side. | (display-list builder) |
| `fix/border-radius-clamp` | Clamp border-radius to the box (CSS Backgrounds 3 §5.5). | `border_radius_clamp_tests` |
| `fix/paged-shadow-bounds` | Items starting at the page bottom belong to the next page. | `items_starting_at_the_page_bottom_or_ending_at_its_top_are_not_kept` |
| `fix/image-object-fit` | Honor `object-fit` / `object-position` for block and inline images: size from the intrinsic size (looked up in the `ImageCache` for a tagged placeholder), place by position, clip to the box with `PushClip`/`PopClip`. `Fill` unchanged. | `object_fit_*`, `object_position_*`, `intrinsic_image_size_*` in `display_list.rs` |

Nothing is open in this list; remaining gaps (`grid-column-start`, 3D
transforms, inset shadows) are in `docs/spikes/003-style-coverage.md`.
