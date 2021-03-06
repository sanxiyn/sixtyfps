/* LICENSE BEGIN
    This file is part of the SixtyFPS Project -- https://sixtyfps.io
    Copyright (c) 2020 Olivier Goffart <olivier.goffart@sixtyfps.io>
    Copyright (c) 2020 Simon Hausmann <simon.hausmann@sixtyfps.io>

    SPDX-License-Identifier: GPL-3.0-only
    This file is also available under commercial licensing terms.
    Please contact info@sixtyfps.io for more information.
LICENSE END */
/*!
This module contains the builtin image related items.

When adding an item or a property, it needs to be kept in sync with different place.
(This is less than ideal and maybe we can have some automation later)

 - It needs to be changed in this module
 - In the compiler: builtins.60
 - In the interpreter: dynamic_component.rs
 - For the C++ code (new item only): the cbindgen.rs to export the new item, and the `using` declaration in sixtyfps.h
 - Don't forget to update the documentation
*/
use super::{Item, ItemConsts, ItemRc};
use crate::eventloop::ComponentWindow;
use crate::graphics::{HighLevelRenderingPrimitive, IntRect, Rect, RenderingVariables, Resource};
use crate::input::{FocusEvent, InputEventResult, KeyEvent, KeyEventResult, MouseEvent};
use crate::item_rendering::CachedRenderingData;
use crate::layout::LayoutInfo;
#[cfg(feature = "rtti")]
use crate::rtti::*;
#[cfg(feature = "rtti")]
use crate::Callback;
use crate::Property;
use const_field_offset::FieldOffsets;
use core::pin::Pin;
use sixtyfps_corelib_macros::*;

#[derive(Copy, Clone, Debug, PartialEq, strum_macros::EnumString, strum_macros::Display)]
#[repr(C)]
#[allow(non_camel_case_types)]
pub enum ImageFit {
    fill,
    contain,
}

impl Default for ImageFit {
    fn default() -> Self {
        ImageFit::fill
    }
}

#[repr(C)]
#[derive(FieldOffsets, Default, BuiltinItem)]
#[pin]
/// The implementation of the `Image` element
pub struct Image {
    pub source: Property<Resource>,
    pub x: Property<f32>,
    pub y: Property<f32>,
    pub width: Property<f32>,
    pub height: Property<f32>,
    pub image_fit: Property<ImageFit>,
    pub cached_rendering_data: CachedRenderingData,
}

impl Item for Image {
    fn init(self: Pin<&Self>, _window: &ComponentWindow) {}

    fn geometry(self: Pin<&Self>) -> Rect {
        euclid::rect(
            Self::FIELD_OFFSETS.x.apply_pin(self).get(),
            Self::FIELD_OFFSETS.y.apply_pin(self).get(),
            Self::FIELD_OFFSETS.width.apply_pin(self).get(),
            Self::FIELD_OFFSETS.height.apply_pin(self).get(),
        )
    }
    fn rendering_primitive(
        self: Pin<&Self>,
        _window: &ComponentWindow,
    ) -> HighLevelRenderingPrimitive {
        HighLevelRenderingPrimitive::Image {
            source: Self::FIELD_OFFSETS.source.apply_pin(self).get(),
            source_clip_rect: IntRect::default(),
        }
    }

    fn rendering_variables(self: Pin<&Self>, _window: &ComponentWindow) -> RenderingVariables {
        RenderingVariables::Image {
            scaled_width: Self::FIELD_OFFSETS.width.apply_pin(self).get(),
            scaled_height: Self::FIELD_OFFSETS.height.apply_pin(self).get(),
            fit: Self::FIELD_OFFSETS.image_fit.apply_pin(self).get(),
        }
    }

    fn layouting_info(self: Pin<&Self>, _window: &ComponentWindow) -> LayoutInfo {
        // FIXME: should we use the image size here
        Default::default()
    }

    fn input_event(
        self: Pin<&Self>,
        _: MouseEvent,
        _window: &ComponentWindow,
        _self_rc: &ItemRc,
    ) -> InputEventResult {
        InputEventResult::EventIgnored
    }

    fn key_event(self: Pin<&Self>, _: &KeyEvent, _window: &ComponentWindow) -> KeyEventResult {
        KeyEventResult::EventIgnored
    }

    fn focus_event(self: Pin<&Self>, _: &FocusEvent, _window: &ComponentWindow) {}
}

impl ItemConsts for Image {
    const cached_rendering_data_offset: const_field_offset::FieldOffset<
        Image,
        CachedRenderingData,
    > = Image::FIELD_OFFSETS.cached_rendering_data.as_unpinned_projection();
}

#[repr(C)]
#[derive(FieldOffsets, Default, BuiltinItem)]
#[pin]
/// The implementation of the `ClippedImage` element
pub struct ClippedImage {
    pub source: Property<Resource>,
    pub x: Property<f32>,
    pub y: Property<f32>,
    pub width: Property<f32>,
    pub height: Property<f32>,
    pub image_fit: Property<ImageFit>,
    pub source_clip_x: Property<i32>,
    pub source_clip_y: Property<i32>,
    pub source_clip_width: Property<i32>,
    pub source_clip_height: Property<i32>,
    pub cached_rendering_data: CachedRenderingData,
}

impl Item for ClippedImage {
    fn init(self: Pin<&Self>, _window: &ComponentWindow) {}

    fn geometry(self: Pin<&Self>) -> Rect {
        euclid::rect(
            Self::FIELD_OFFSETS.x.apply_pin(self).get(),
            Self::FIELD_OFFSETS.y.apply_pin(self).get(),
            Self::FIELD_OFFSETS.width.apply_pin(self).get(),
            Self::FIELD_OFFSETS.height.apply_pin(self).get(),
        )
    }
    fn rendering_primitive(
        self: Pin<&Self>,
        _window: &ComponentWindow,
    ) -> HighLevelRenderingPrimitive {
        HighLevelRenderingPrimitive::Image {
            source: Self::FIELD_OFFSETS.source.apply_pin(self).get(),
            source_clip_rect: euclid::rect(
                Self::FIELD_OFFSETS.source_clip_x.apply_pin(self).get(),
                Self::FIELD_OFFSETS.source_clip_y.apply_pin(self).get(),
                Self::FIELD_OFFSETS.source_clip_width.apply_pin(self).get(),
                Self::FIELD_OFFSETS.source_clip_height.apply_pin(self).get(),
            ),
        }
    }

    fn rendering_variables(self: Pin<&Self>, _window: &ComponentWindow) -> RenderingVariables {
        RenderingVariables::Image {
            scaled_width: Self::FIELD_OFFSETS.width.apply_pin(self).get(),
            scaled_height: Self::FIELD_OFFSETS.height.apply_pin(self).get(),
            fit: Self::FIELD_OFFSETS.image_fit.apply_pin(self).get(),
        }
    }

    fn layouting_info(self: Pin<&Self>, _window: &ComponentWindow) -> LayoutInfo {
        // FIXME: should we use the image size here
        Default::default()
    }

    fn input_event(
        self: Pin<&Self>,
        _: MouseEvent,
        _window: &ComponentWindow,
        _self_rc: &ItemRc,
    ) -> InputEventResult {
        InputEventResult::EventIgnored
    }

    fn key_event(self: Pin<&Self>, _: &KeyEvent, _window: &ComponentWindow) -> KeyEventResult {
        KeyEventResult::EventIgnored
    }

    fn focus_event(self: Pin<&Self>, _: &FocusEvent, _window: &ComponentWindow) {}
}

impl ItemConsts for ClippedImage {
    const cached_rendering_data_offset: const_field_offset::FieldOffset<
        ClippedImage,
        CachedRenderingData,
    > = ClippedImage::FIELD_OFFSETS.cached_rendering_data.as_unpinned_projection();
}
