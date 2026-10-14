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
| `fix/img-intrinsic-size` | Pass azul an `ImageCache` naming each embedded image's decoded size (layout only sees a tagged 0x0 placeholder for `<img>`). Needed by azul's object-fit. | `bridge::tests::image_cache_names_each_images_decoded_size` |

Note for the maintainer: an earlier attempt gave `<img>` `width`/`height`
attributes from the decoded size. That changes layout (the intrinsic width
inflates percentage-width containers), so it was reverted in favor of the cache.

## fschutt/azul

| Branch | Change | Test |
|---|---|---|
| `fix/paged-opacity` | `PushOpacity`/`PopOpacity` become structure markers in the paginator (`is_push_marker`, `is_pop_marker`, `matching_pop`, `rebase_marker`), so opacity survives page splitting in paged output. | `paginate_keeps_opacity_markers_balanced_on_every_page` (fails without the fix), `paginate_omits_opacity_markers_from_pages_without_their_content` |
| `fix/feature-gating` | `azul-layout` on `master` did not build with `default-features = false` (`rasterize_svg_clip_to_r8` uses `agg_rust`; `tile_mask` is `cpurender`-only but its caller was ungated). printpdf builds it that way. | `cargo check -p azul-layout --no-default-features` |
| `fix/image-object-fit` | Honor `object-fit` / `object-position` for block and inline images: size from the intrinsic size (looked up in the `ImageCache` for a tagged placeholder), place by position, clip to the box with `PushClip`/`PopClip`. `Fill` unchanged. | `object_fit_*`, `object_position_*`, `intrinsic_image_size_*` in `display_list.rs` |

Open (not fixed): `vertical-align` on inline spans, `transform`, `text-shadow`;
see `docs/spikes/003-style-coverage.md`.
