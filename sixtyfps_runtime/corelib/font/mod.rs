/* LICENSE BEGIN
    This file is part of the SixtyFPS Project -- https://sixtyfps.io
    Copyright (c) 2020 Olivier Goffart <olivier.goffart@sixtyfps.io>
    Copyright (c) 2020 Simon Hausmann <simon.hausmann@sixtyfps.io>

    SPDX-License-Identifier: GPL-3.0-only
    This file is also available under commercial licensing terms.
    Please contact info@sixtyfps.io for more information.
LICENSE END */
/*!
Font abstraction for the run-time library.

The module receives FontRequest objects and returns Rc<Font>, which represents a font provided by
the underlying platform that matches the specified font request as closely as possible.

Internally a FontRequest is resolved to a Rc<PlatformFont> and a list of Rc<Font> instances, one for
each pixel size. The Rc<Font> is basically an Rc<PlatformFont> and the pixel size specific, cached glyph
metrics -- base on the assumption that the platform provides scalable fonts.

On the graphics side, the generated rasterized glyphs may be cached in textures. That cache is indexed by the
Rc<PlatformFont> since the underlying platform may map different font requests to the same physical PlatformFont
(typically backed by a .ttf file or ttf inside a .ttc)
*/
use crate::string::SharedString;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

#[cfg(not(target_arch = "wasm32"))]
mod fontkit;
#[cfg(not(target_arch = "wasm32"))]
pub use fontkit::*;

#[cfg(target_arch = "wasm32")]
mod canvasfont;
#[cfg(target_arch = "wasm32")]
pub use canvasfont::*;

/// GlyphMetrics contains the different kinds of measures for glyphs. This is typically obtained
/// using the Font APIs.
#[derive(Clone)]
pub struct GlyphMetrics {
    /// The distance from this glyph to the next one.
    pub advance: f32,
}

struct FontMatch {
    handle: Rc<PlatformFont>,
    fonts_per_pixel_size: Vec<Rc<Font>>,
}

impl FontMatch {
    fn new(handle: Rc<PlatformFont>) -> Self {
        Self { handle, fonts_per_pixel_size: Vec::new() }
    }
}

/// FontRequest collects all the developer-configurable properties for fonts, such as family, weight, etc.
/// It is submitted as a request to the platform font system (i.e. CoreText on macOS) and in exchange we
/// store a Rc<FontHandle>
#[derive(Debug, Clone, PartialEq)]
#[repr(C)]
pub struct FontRequest {
    family: SharedString,
    weight: i32,
    pixel_size: f32,
}

/// HasFont is a convenience trait for items holding font properties, such as Text or TextInput.
pub trait HasFont {
    /// Return the value of the font-family property.
    fn font_family(&self) -> SharedString;
    /// Return the value of the font-weight property.
    fn font_weight(&self) -> i32;
    /// Return the value if the font-size property converted to window specific pixels, respecting
    /// the window scale factor.
    fn font_pixel_size(&self, window: &crate::eventloop::ComponentWindow) -> f32;
    /// Translates the values of the different font related properties into a FontRequest object.
    fn font_request(&self, window: &crate::eventloop::ComponentWindow) -> FontRequest {
        FontRequest {
            family: self.font_family(),
            weight: self.font_weight(),
            pixel_size: self.font_pixel_size(window),
        }
    }
    /// Returns a Font object that matches the requested font properties of this trait object (item).
    fn font(&self, window: &crate::eventloop::ComponentWindow) -> Rc<Font> {
        crate::font::FONT_CACHE.with(|fc| fc.find_font(&self.font_request(window)))
    }
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct FontCacheKey {
    family: SharedString,
    weight: i32,
}

impl FontCacheKey {
    fn new(request: &FontRequest) -> Self {
        Self { family: request.family.clone(), weight: request.weight }
    }
}

/// FontCache caches the expensive process of looking up fonts by family, weight, style, etc. (FontRequest)
#[derive(Default)]
pub struct FontCache {
    // index by family name
    loaded_fonts: RefCell<HashMap<FontCacheKey, FontMatch>>,
    application_fonts: RefCell<HashMap<String, FontMatch>>,
}

impl FontCache {
    /// Submits the given FontRequest to the platform's font system (i.e. CoreText) and returns the font found.
    /// The result is cached, so this function should be cheap to call.
    pub fn find_font(&self, request: &FontRequest) -> Rc<Font> {
        assert_ne!(request.pixel_size, 0.0);

        let mut loaded_fonts = self.loaded_fonts.borrow_mut();
        let mut application_fonts = self.application_fonts.borrow_mut();

        let font_match = application_fonts.get_mut(request.family.as_str()).unwrap_or_else(|| {
            loaded_fonts.entry(FontCacheKey::new(request)).or_insert_with(|| {
                FontMatch::new(PlatformFont::new_from_request(&request).unwrap())
            })
        });

        font_match
            .fonts_per_pixel_size
            .iter()
            .find_map(
                |font| {
                    if font.pixel_size == request.pixel_size {
                        Some(font.clone())
                    } else {
                        None
                    }
                },
            )
            .unwrap_or_else(|| {
                let fnt = Rc::new(font_match.handle.load(request.pixel_size));
                font_match.fonts_per_pixel_size.push(fnt.clone());
                fnt
            })
    }
}

thread_local! {
    /// The thread-local font-cache holding references to resolved font requests
    pub static FONT_CACHE: FontCache = Default::default();
}

/// This function can be used to register a custom TrueType font with SixtyFPS,
/// for use with the `font-family` property. The provided slice must be a valid TrueType
/// font.
#[cfg(not(target_arch = "wasm32"))]
pub fn register_application_font_from_memory(
    data: &'static [u8],
) -> Result<(), Box<dyn std::error::Error>> {
    let platform_font = PlatformFont::new_from_slice(data).map_err(Box::new)?;

    FONT_CACHE.with(|fc| {
        fc.application_fonts
            .borrow_mut()
            .insert(platform_font.family_name(), FontMatch::new(platform_font))
    });

    Ok(())
}
