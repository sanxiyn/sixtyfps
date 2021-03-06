/* LICENSE BEGIN
    This file is part of the SixtyFPS Project -- https://sixtyfps.io
    Copyright (c) 2020 Olivier Goffart <olivier.goffart@sixtyfps.io>
    Copyright (c) 2020 Simon Hausmann <simon.hausmann@sixtyfps.io>

    SPDX-License-Identifier: GPL-3.0-only
    This file is also available under commercial licensing terms.
    Please contact info@sixtyfps.io for more information.
LICENSE END */
use super::buffers::GLArrayBuffer;
use super::texture::{AtlasAllocation, TextureAtlas};
use super::Vertex;
use collections::hash_map::HashMap;
use itertools::Itertools;
use sixtyfps_corelib::font::{Font, FontRequest, PlatformFont};
use std::cell::RefCell;
use std::{collections, rc::Rc};

type GlyphsByPixelSize = Vec<Rc<RefCell<CachedFontGlyphs>>>;

use super::GlyphRun;

#[derive(Default)]
pub(crate) struct GlyphCache {
    glyphs_by_font: RefCell<HashMap<Rc<PlatformFont>, GlyphsByPixelSize>>,
}

impl GlyphCache {
    pub fn find_font(&self, request: &FontRequest) -> Rc<RefCell<CachedFontGlyphs>> {
        let font = sixtyfps_corelib::font::FONT_CACHE.with(|fc| fc.find_font(request));

        let font_handle = font.handle();

        let mut glyphs_by_font = self.glyphs_by_font.borrow_mut();
        let glyphs_by_pixel_size =
            glyphs_by_font.entry(font_handle.clone()).or_insert(GlyphsByPixelSize::default());

        glyphs_by_pixel_size
            .iter()
            .find_map(|gl_font| {
                if gl_font.borrow().font.pixel_size == font.pixel_size {
                    Some(gl_font.clone())
                } else {
                    None
                }
            })
            .unwrap_or_else(|| {
                let fnt = Rc::new(RefCell::new(CachedFontGlyphs::new(font.clone())));
                glyphs_by_pixel_size.push(fnt.clone());
                fnt
            })
    }
}

pub struct PreRenderedGlyph {
    pub glyph_allocation: Option<AtlasAllocation>,
    pub advance: f32,
    pub x: f32,
    pub y: f32,
}

pub struct CachedFontGlyphs {
    pub font: Rc<Font>,
    glyphs: HashMap<u32, PreRenderedGlyph>,
}

impl CachedFontGlyphs {
    pub fn new(font: Rc<Font>) -> Self {
        let glyphs = HashMap::new();
        Self { font, glyphs }
    }

    pub fn layout_glyphs<'a>(
        &'a mut self,
        gl: &'a Rc<glow::Context>,
        atlas: &'a mut TextureAtlas,
        text: &'a str,
    ) -> impl Iterator<Item = &PreRenderedGlyph> + 'a {
        let glyphs = self
            .font
            .clone()
            .string_to_glyphs(text)
            .map(|(_, ch, glyph_id)| (ch, glyph_id))
            .collect::<smallvec::SmallVec<[(_, _); 32]>>();

        glyphs.iter().for_each(|(ch, glyph)| {
            if !self.glyphs.contains_key(&glyph) {
                // ensure the glyph is cached
                self.glyphs.insert(*glyph, self.render_glyph(gl, atlas, *ch, *glyph));
            }
        });

        GlyphIter { gl_font: self, glyph_it: glyphs.into_iter().map(|(_, g)| g) }
    }

    fn render_glyph(
        &self,
        gl: &Rc<glow::Context>,
        atlas: &mut TextureAtlas,
        ch: char,
        glyph_id: u32,
    ) -> PreRenderedGlyph {
        let advance = self.font.glyph_metrics(glyph_id).advance;

        let (x, y, glyph_allocation) = if !ch.is_whitespace() {
            let (x, y, glyph_image) = self.font.rasterize_glyph(glyph_id);

            (
                x,
                y,
                Some(
                    atlas.allocate_image_in_atlas(
                        gl,
                        image::ImageBuffer::<_, &[u8]>::from_raw(
                            glyph_image.width(),
                            glyph_image.height(),
                            &glyph_image,
                        )
                        .unwrap(),
                    ),
                ),
            )
        } else {
            (0., 0., None)
        };

        PreRenderedGlyph { glyph_allocation, advance, x, y }
    }

    pub fn render_glyphs(
        &mut self,
        context: &Rc<glow::Context>,
        texture_atlas: &mut TextureAtlas,
        text: &str,
    ) -> Vec<GlyphRun> {
        let mut x = 0.;
        let ascent = self.font.ascent();

        self.layout_glyphs(&context, texture_atlas, text)
            .filter_map(|cached_glyph| {
                let glyph_x = x;
                x += cached_glyph.advance;

                if let Some(glyph_allocation) = &cached_glyph.glyph_allocation {
                    let glyph_width = glyph_allocation.texture_coordinates.width() as f32;
                    let glyph_height = glyph_allocation.texture_coordinates.height() as f32;

                    let pen_x = glyph_x + cached_glyph.x;
                    let pen_y = cached_glyph.y + ascent;

                    let vertex1 = Vertex { _pos: [pen_x, pen_y] };
                    let vertex2 = Vertex { _pos: [pen_x + glyph_width, pen_y] };
                    let vertex3 = Vertex { _pos: [pen_x + glyph_width, pen_y + glyph_height] };
                    let vertex4 = Vertex { _pos: [pen_x, pen_y + glyph_height] };

                    let vertices = [vertex1, vertex2, vertex3, vertex1, vertex3, vertex4];
                    let texture_vertices = glyph_allocation.normalized_texture_coordinates();

                    Some((vertices, texture_vertices, glyph_allocation.clone()))
                } else {
                    None
                }
            })
            .group_by(|(_, _, allocation)| allocation.atlas.texture.clone())
            .into_iter()
            .map(|(texture, glyph_it)| {
                let glyph_count = glyph_it.size_hint().0;
                let mut vertices: Vec<Vertex> = Vec::with_capacity(glyph_count * 6);
                let mut texture_vertices: Vec<Vertex> = Vec::with_capacity(glyph_count * 6);

                for (glyph_vertices, glyph_texture_vertices) in
                    glyph_it.map(|(vertices, texture_vertices, _)| (vertices, texture_vertices))
                {
                    vertices.extend(&glyph_vertices);
                    texture_vertices.extend(&glyph_texture_vertices);
                }

                let vertex_count = vertices.len() as i32;
                GlyphRun {
                    vertices: GLArrayBuffer::new(&context, &vertices),
                    texture_vertices: GLArrayBuffer::new(&context, &texture_vertices),
                    texture,
                    vertex_count,
                }
            })
            .collect()
    }
}

pub struct GlyphIter<'a, GlyphIterator> {
    gl_font: &'a CachedFontGlyphs,
    glyph_it: GlyphIterator,
}

impl<'a, GlyphIterator> Iterator for GlyphIter<'a, GlyphIterator>
where
    GlyphIterator: std::iter::Iterator<Item = u32>,
{
    type Item = &'a PreRenderedGlyph;
    fn next(&mut self) -> Option<Self::Item> {
        if let Some(glyph_id) = self.glyph_it.next() {
            Some(&self.gl_font.glyphs[&glyph_id])
        } else {
            None
        }
    }
}
