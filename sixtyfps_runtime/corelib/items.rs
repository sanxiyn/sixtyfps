/* LICENSE BEGIN
    This file is part of the SixtyFPS Project -- https://sixtyfps.io
    Copyright (c) 2020 Olivier Goffart <olivier.goffart@sixtyfps.io>
    Copyright (c) 2020 Simon Hausmann <simon.hausmann@sixtyfps.io>

    SPDX-License-Identifier: GPL-3.0-only
    This file is also available under commercial licensing terms.
    Please contact info@sixtyfps.io for more information.
LICENSE END */
/*!
This module contains the builtin items, either in this file or in sub-modules.

When adding an item or a property, it needs to be kept in sync with different place.
(This is less than ideal and maybe we can have some automation later)

 - It needs to be changed in this module
 - In the compiler: builtins.60
 - In the interpreter (new item only): dynamic_component.rs
 - For the C++ code (new item only): the cbindgen.rs to export the new item, and the `using` declaration in sixtyfps.h
 - Don't forget to update the documentation
*/

#![allow(unsafe_code)]
#![allow(non_upper_case_globals)]
#![allow(missing_docs)] // because documenting each property of items is redundent

use super::eventloop::ComponentWindow;
use super::graphics::{Color, HighLevelRenderingPrimitive, PathData, Rect};
use super::input::{
    FocusEvent, InputEventResult, KeyEvent, KeyEventResult, MouseEvent, MouseEventType,
};
use super::item_rendering::CachedRenderingData;
use super::layout::LayoutInfo;
use crate::component::ComponentVTable;
#[cfg(feature = "rtti")]
use crate::{rtti::*, SharedString};
use crate::{Callback, Property};
use const_field_offset::FieldOffsets;
use core::pin::Pin;
use sixtyfps_corelib_macros::*;
use vtable::*;

mod text;
pub use text::*;
mod image;
pub use self::image::*;

/// Items are the nodes in the render tree.
#[vtable]
#[repr(C)]
pub struct ItemVTable {
    /// This function is called by the run-time after the memory for the item
    /// has been allocated and initialized. It will be called before any user specified
    /// bindings are set.
    pub init: extern "C" fn(core::pin::Pin<VRef<ItemVTable>>, window: &ComponentWindow),

    /// Returns the geometry of this item (relative to its parent item)
    pub geometry: extern "C" fn(core::pin::Pin<VRef<ItemVTable>>) -> Rect,

    /// offset in bytes fromthe *const ItemImpl.
    /// isize::MAX  means None
    #[allow(non_upper_case_globals)]
    #[field_offset(CachedRenderingData)]
    pub cached_rendering_data_offset: usize,

    /// Return the rendering primitive used to display this item. This should depend on only
    /// rarely changed properties as it typically contains data uploaded to the GPU.
    pub rendering_primitive: extern "C" fn(
        core::pin::Pin<VRef<ItemVTable>>,
        window: &ComponentWindow,
    ) -> HighLevelRenderingPrimitive,

    /// Return the variables needed to render the graphical primitives of this item. These
    /// are typically variables that do not require uploading any data sets to the GPU and
    /// can instead be represented using uniforms.
    pub rendering_variables: extern "C" fn(
        core::pin::Pin<VRef<ItemVTable>>,
        window: &ComponentWindow,
    ) -> RenderingVariables,

    /// We would need max/min/preferred size, and all layout info
    pub layouting_info:
        extern "C" fn(core::pin::Pin<VRef<ItemVTable>>, window: &ComponentWindow) -> LayoutInfo,

    /// input event
    pub input_event: extern "C" fn(
        core::pin::Pin<VRef<ItemVTable>>,
        MouseEvent,
        window: &ComponentWindow,
        self_rc: &ItemRc,
    ) -> InputEventResult,

    pub focus_event:
        extern "C" fn(core::pin::Pin<VRef<ItemVTable>>, &FocusEvent, window: &ComponentWindow),

    pub key_event: extern "C" fn(
        core::pin::Pin<VRef<ItemVTable>>,
        &KeyEvent,
        window: &ComponentWindow,
    ) -> KeyEventResult,
}

/// Alias for `vtable::VRef<ItemVTable>` which represent a pointer to a `dyn Item` with
/// the associated vtable
pub type ItemRef<'a> = vtable::VRef<'a, ItemVTable>;

/// A ItemRc is holding a reference to a component containing the item, and the index of this item
#[repr(C)]
#[derive(Clone)]
pub struct ItemRc {
    component: vtable::VRc<ComponentVTable>,
    index: usize,
}

impl ItemRc {
    /// Create an ItemRc from a component and an index
    pub fn new(component: vtable::VRc<ComponentVTable>, index: usize) -> Self {
        Self { component, index }
    }
    /// Return a `Pin<ItemRef<'a>>`
    pub fn borrow<'a>(&'a self) -> Pin<ItemRef<'a>> {
        let comp_ref_pin = vtable::VRc::borrow_pin(&self.component);
        let result = comp_ref_pin.as_ref().get_item_ref(self.index);
        // Safety: we can expand the lifetime of the ItemRef because we know it lives for at least the
        // lifetime of the component, which is 'a.  Pin::as_ref removes the lifetime, but we can just put it back.
        unsafe { core::mem::transmute::<Pin<ItemRef<'_>>, Pin<ItemRef<'a>>>(result) }
    }
    pub fn downgrade(&self) -> ItemWeak {
        ItemWeak { component: VRc::downgrade(&self.component), index: self.index }
    }
}

/// A Weak reference to an item that can be constructed from an ItemRc.
#[derive(Default, Clone)]
pub struct ItemWeak {
    component: crate::component::ComponentWeak,
    index: usize,
}

impl ItemWeak {
    pub fn upgrade(&self) -> Option<ItemRc> {
        self.component.upgrade().map(|c| ItemRc::new(c, self.index))
    }
}

#[repr(C)]
#[derive(FieldOffsets, Default, BuiltinItem)]
#[pin]
/// The implementation of the `Rectangle` element
pub struct Rectangle {
    pub color: Property<Color>,
    pub x: Property<f32>,
    pub y: Property<f32>,
    pub width: Property<f32>,
    pub height: Property<f32>,
    pub cached_rendering_data: CachedRenderingData,
}

impl Item for Rectangle {
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
        let width = Self::FIELD_OFFSETS.width.apply_pin(self).get();
        let height = Self::FIELD_OFFSETS.height.apply_pin(self).get();
        if width > 0. && height > 0. {
            HighLevelRenderingPrimitive::Rectangle { width, height }
        } else {
            HighLevelRenderingPrimitive::NoContents
        }
    }

    fn rendering_variables(self: Pin<&Self>, _window: &ComponentWindow) -> RenderingVariables {
        RenderingVariables::Rectangle {
            fill: Self::FIELD_OFFSETS.color.apply_pin(self).get(),
            stroke: Color::default(),
            border_width: 0.,
            border_radius: 0.,
        }
    }

    fn layouting_info(self: Pin<&Self>, _window: &crate::eventloop::ComponentWindow) -> LayoutInfo {
        LayoutInfo { horizontal_stretch: 1., vertical_stretch: 1., ..LayoutInfo::default() }
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

impl ItemConsts for Rectangle {
    const cached_rendering_data_offset: const_field_offset::FieldOffset<
        Rectangle,
        CachedRenderingData,
    > = Rectangle::FIELD_OFFSETS.cached_rendering_data.as_unpinned_projection();
}

ItemVTable_static! {
    /// The VTable for `Rectangle`
    #[no_mangle]
    pub static RectangleVTable for Rectangle
}

#[repr(C)]
#[derive(FieldOffsets, Default, BuiltinItem)]
#[pin]
/// The implementation of the `BorderRectangle` element
pub struct BorderRectangle {
    pub color: Property<Color>,
    pub x: Property<f32>,
    pub y: Property<f32>,
    pub width: Property<f32>,
    pub height: Property<f32>,
    pub border_width: Property<f32>,
    pub border_radius: Property<f32>,
    pub border_color: Property<Color>,
    pub cached_rendering_data: CachedRenderingData,
}

impl Item for BorderRectangle {
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
        let width = Self::FIELD_OFFSETS.width.apply_pin(self).get();
        let height = Self::FIELD_OFFSETS.height.apply_pin(self).get();
        if width > 0. && height > 0. {
            HighLevelRenderingPrimitive::Rectangle { width, height }
        } else {
            HighLevelRenderingPrimitive::NoContents
        }
    }

    fn rendering_variables(self: Pin<&Self>, _window: &ComponentWindow) -> RenderingVariables {
        RenderingVariables::Rectangle {
            fill: Self::FIELD_OFFSETS.color.apply_pin(self).get(),
            stroke: Self::FIELD_OFFSETS.border_color.apply_pin(self).get(),
            border_width: Self::FIELD_OFFSETS.border_width.apply_pin(self).get(),
            border_radius: Self::FIELD_OFFSETS.border_radius.apply_pin(self).get(),
        }
    }

    fn layouting_info(self: Pin<&Self>, _window: &crate::eventloop::ComponentWindow) -> LayoutInfo {
        LayoutInfo { horizontal_stretch: 1., vertical_stretch: 1., ..LayoutInfo::default() }
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

impl ItemConsts for BorderRectangle {
    const cached_rendering_data_offset: const_field_offset::FieldOffset<
        BorderRectangle,
        CachedRenderingData,
    > = BorderRectangle::FIELD_OFFSETS.cached_rendering_data.as_unpinned_projection();
}

ItemVTable_static! {
    /// The VTable for `BorderRectangle`
    #[no_mangle]
    pub static BorderRectangleVTable for BorderRectangle
}

ItemVTable_static! {
    /// The VTable for `Image`
    #[no_mangle]
    pub static ImageVTable for Image
}

ItemVTable_static! {
    /// The VTable for `ClippedImage`
    #[no_mangle]
    pub static ClippedImageVTable for ClippedImage
}

/// The implementation of the `TouchArea` element
#[repr(C)]
#[derive(FieldOffsets, Default, BuiltinItem)]
#[pin]
pub struct TouchArea {
    pub x: Property<f32>,
    pub y: Property<f32>,
    pub width: Property<f32>,
    pub height: Property<f32>,
    /// FIXME: We should anotate this as an "output" property.
    pub pressed: Property<bool>,
    pub has_hover: Property<bool>,
    /// FIXME: there should be just one property for the point istead of two.
    /// Could even be merged with pressed in a Property<Option<Point>> (of course, in the
    /// implementation item only, for the compiler it would stay separate properties)
    pub pressed_x: Property<f32>,
    pub pressed_y: Property<f32>,
    /// FIXME: should maybe be as parameter to the mouse event instead. Or at least just one property
    pub mouse_x: Property<f32>,
    pub mouse_y: Property<f32>,
    pub clicked: Callback<()>,
    /// FIXME: remove this
    pub cached_rendering_data: CachedRenderingData,
}

impl Item for TouchArea {
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
        HighLevelRenderingPrimitive::NoContents
    }

    fn rendering_variables(self: Pin<&Self>, _window: &ComponentWindow) -> RenderingVariables {
        RenderingVariables::default()
    }

    fn layouting_info(self: Pin<&Self>, _window: &ComponentWindow) -> LayoutInfo {
        LayoutInfo::default()
    }

    fn input_event(
        self: Pin<&Self>,
        event: MouseEvent,
        _window: &ComponentWindow,
        _self_rc: &ItemRc,
    ) -> InputEventResult {
        Self::FIELD_OFFSETS.mouse_x.apply_pin(self).set(event.pos.x);
        Self::FIELD_OFFSETS.mouse_y.apply_pin(self).set(event.pos.y);
        Self::FIELD_OFFSETS.has_hover.apply_pin(self).set(event.what != MouseEventType::MouseExit);

        let result = if matches!(event.what, MouseEventType::MouseReleased) {
            Self::FIELD_OFFSETS.clicked.apply_pin(self).emit(&());
            InputEventResult::EventAccepted
        } else {
            InputEventResult::GrabMouse
        };

        Self::FIELD_OFFSETS.pressed.apply_pin(self).set(match event.what {
            MouseEventType::MousePressed => {
                Self::FIELD_OFFSETS.pressed_x.apply_pin(self).set(event.pos.x);
                Self::FIELD_OFFSETS.pressed_y.apply_pin(self).set(event.pos.y);
                true
            }
            MouseEventType::MouseExit | MouseEventType::MouseReleased => false,
            MouseEventType::MouseMoved => {
                return if Self::FIELD_OFFSETS.pressed.apply_pin(self).get() {
                    InputEventResult::GrabMouse
                } else {
                    InputEventResult::ObserveHover
                }
            }
        });
        result
    }

    fn key_event(self: Pin<&Self>, _: &KeyEvent, _window: &ComponentWindow) -> KeyEventResult {
        KeyEventResult::EventIgnored
    }

    fn focus_event(self: Pin<&Self>, _: &FocusEvent, _window: &ComponentWindow) {}
}

impl ItemConsts for TouchArea {
    const cached_rendering_data_offset: const_field_offset::FieldOffset<
        TouchArea,
        CachedRenderingData,
    > = TouchArea::FIELD_OFFSETS.cached_rendering_data.as_unpinned_projection();
}

ItemVTable_static! {
    /// The VTable for `TouchArea`
    #[no_mangle]
    pub static TouchAreaVTable for TouchArea
}

#[repr(C)]
#[derive(FieldOffsets, Default, BuiltinItem)]
#[pin]
/// The implementation of the `Rectangle` element
pub struct Clip {
    pub color: Property<Color>,
    pub x: Property<f32>,
    pub y: Property<f32>,
    pub width: Property<f32>,
    pub height: Property<f32>,
    pub cached_rendering_data: CachedRenderingData,
}

impl Item for Clip {
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
        let width = Self::FIELD_OFFSETS.width.apply_pin(self).get();
        let height = Self::FIELD_OFFSETS.height.apply_pin(self).get();
        HighLevelRenderingPrimitive::ClipRect { width, height }
    }

    fn rendering_variables(self: Pin<&Self>, _window: &ComponentWindow) -> RenderingVariables {
        Default::default()
    }

    fn layouting_info(self: Pin<&Self>, _window: &crate::eventloop::ComponentWindow) -> LayoutInfo {
        LayoutInfo { horizontal_stretch: 1., vertical_stretch: 1., ..LayoutInfo::default() }
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

impl ItemConsts for Clip {
    const cached_rendering_data_offset: const_field_offset::FieldOffset<Clip, CachedRenderingData> =
        Clip::FIELD_OFFSETS.cached_rendering_data.as_unpinned_projection();
}

ItemVTable_static! {
    /// The VTable for `Clip`
    #[no_mangle]
    pub static ClipVTable for Clip
}

/// The implementation of the `Path` element
#[repr(C)]
#[derive(FieldOffsets, Default, BuiltinItem)]
#[pin]
pub struct Path {
    pub x: Property<f32>,
    pub y: Property<f32>,
    pub width: Property<f32>,
    pub height: Property<f32>,
    pub elements: Property<PathData>,
    pub fill_color: Property<Color>,
    pub stroke_color: Property<Color>,
    pub stroke_width: Property<f32>,
    pub cached_rendering_data: CachedRenderingData,
}

impl Item for Path {
    fn init(self: Pin<&Self>, _window: &ComponentWindow) {}

    fn geometry(self: Pin<&Self>) -> Rect {
        euclid::rect(
            Self::FIELD_OFFSETS.x.apply_pin(self).get(),
            Self::FIELD_OFFSETS.y.apply_pin(self).get(),
            0.,
            0.,
        )
    }
    fn rendering_primitive(
        self: Pin<&Self>,
        _window: &ComponentWindow,
    ) -> HighLevelRenderingPrimitive {
        HighLevelRenderingPrimitive::Path {
            width: Self::FIELD_OFFSETS.width.apply_pin(self).get(),
            height: Self::FIELD_OFFSETS.height.apply_pin(self).get(),
            elements: Self::FIELD_OFFSETS.elements.apply_pin(self).get(),
            stroke_width: Self::FIELD_OFFSETS.stroke_width.apply_pin(self).get(),
        }
    }

    fn rendering_variables(self: Pin<&Self>, _window: &ComponentWindow) -> RenderingVariables {
        RenderingVariables::Path {
            fill: Self::FIELD_OFFSETS.fill_color.apply_pin(self).get(),
            stroke: Self::FIELD_OFFSETS.stroke_color.apply_pin(self).get(),
        }
    }

    fn layouting_info(self: Pin<&Self>, _window: &ComponentWindow) -> LayoutInfo {
        LayoutInfo::default()
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

impl ItemConsts for Path {
    const cached_rendering_data_offset: const_field_offset::FieldOffset<Path, CachedRenderingData> =
        Path::FIELD_OFFSETS.cached_rendering_data.as_unpinned_projection();
}

ItemVTable_static! {
    /// The VTable for `Path`
    #[no_mangle]
    pub static PathVTable for Path
}

/// The implementation of the `Flickable` element
#[repr(C)]
#[derive(FieldOffsets, Default, BuiltinItem)]
#[pin]
pub struct Flickable {
    pub x: Property<f32>,
    pub y: Property<f32>,
    pub width: Property<f32>,
    pub height: Property<f32>,
    pub viewport: Rectangle,
    pub interactive: Property<bool>,
    data: FlickableDataBox,

    /// FIXME: remove this
    pub cached_rendering_data: CachedRenderingData,
}

impl Item for Flickable {
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
        HighLevelRenderingPrimitive::ClipRect {
            width: Self::FIELD_OFFSETS.width.apply_pin(self).get(),
            height: Self::FIELD_OFFSETS.height.apply_pin(self).get(),
        }
    }

    fn rendering_variables(self: Pin<&Self>, _window: &ComponentWindow) -> RenderingVariables {
        Default::default()
    }

    fn layouting_info(self: Pin<&Self>, _window: &ComponentWindow) -> LayoutInfo {
        LayoutInfo::default()
    }

    fn input_event(
        self: Pin<&Self>,
        event: MouseEvent,
        _window: &ComponentWindow,
        _self_rc: &ItemRc,
    ) -> InputEventResult {
        if !Self::FIELD_OFFSETS.interactive.apply_pin(self).get() {
            return InputEventResult::EventIgnored;
        }
        self.data.handle_mouse(self, event);

        if event.what == MouseEventType::MousePressed || event.what == MouseEventType::MouseMoved {
            // FIXME
            InputEventResult::GrabMouse
        } else {
            InputEventResult::EventAccepted
        }
    }

    fn key_event(self: Pin<&Self>, _: &KeyEvent, _window: &ComponentWindow) -> KeyEventResult {
        KeyEventResult::EventIgnored
    }

    fn focus_event(self: Pin<&Self>, _: &FocusEvent, _window: &ComponentWindow) {}
}

impl ItemConsts for Flickable {
    const cached_rendering_data_offset: const_field_offset::FieldOffset<Self, CachedRenderingData> =
        Self::FIELD_OFFSETS.cached_rendering_data.as_unpinned_projection();
}

ItemVTable_static! {
    /// The VTable for `Flickable`
    #[no_mangle]
    pub static FlickableVTable for Flickable
}

pub use crate::{graphics::RenderingVariables, SharedVector};

#[repr(C)]
/// Wraps the internal datastructure for the Flickable
pub struct FlickableDataBox(core::ptr::NonNull<crate::flickable::FlickableData>);

impl Default for FlickableDataBox {
    fn default() -> Self {
        FlickableDataBox(Box::leak(Box::new(crate::flickable::FlickableData::default())).into())
    }
}
impl Drop for FlickableDataBox {
    fn drop(&mut self) {
        // Safety: the self.0 was constructed from a Box::leak in FlickableDataBox::default
        unsafe {
            Box::from_raw(self.0.as_ptr());
        }
    }
}
impl core::ops::Deref for FlickableDataBox {
    type Target = crate::flickable::FlickableData;
    fn deref(&self) -> &Self::Target {
        // Safety: initialized in FlickableDataBox::default
        unsafe { self.0.as_ref() }
    }
}

#[no_mangle]
pub unsafe extern "C" fn sixtyfps_flickable_data_init(data: *mut FlickableDataBox) {
    std::ptr::write(data, FlickableDataBox::default());
}
#[no_mangle]
pub unsafe extern "C" fn sixtyfps_flickable_data_free(data: *mut FlickableDataBox) {
    std::ptr::read(data);
}

/// The implementation of the `PropertyAnimation` element
#[repr(C)]
#[derive(FieldOffsets, Default, BuiltinItem, Clone, Debug)]
#[pin]
pub struct PropertyAnimation {
    #[rtti_field]
    pub duration: i32,
    #[rtti_field]
    pub loop_count: i32,
    #[rtti_field]
    pub easing: crate::animations::EasingCurve,
}

/// The implementation of the `Window` element
#[repr(C)]
#[derive(FieldOffsets, Default, BuiltinItem)]
#[pin]
pub struct Window {
    pub width: Property<f32>,
    pub height: Property<f32>,
    pub color: Property<Color>,
    pub title: Property<SharedString>,
    pub cached_rendering_data: CachedRenderingData,
}

impl Item for Window {
    fn init(self: Pin<&Self>, _window: &ComponentWindow) {}

    fn geometry(self: Pin<&Self>) -> Rect {
        euclid::rect(
            0.,
            0.,
            Self::FIELD_OFFSETS.width.apply_pin(self).get(),
            Self::FIELD_OFFSETS.height.apply_pin(self).get(),
        )
    }
    fn rendering_primitive(
        self: Pin<&Self>,
        _window: &ComponentWindow,
    ) -> HighLevelRenderingPrimitive {
        HighLevelRenderingPrimitive::NoContents
    }

    fn rendering_variables(self: Pin<&Self>, _window: &ComponentWindow) -> RenderingVariables {
        Default::default()
    }

    fn layouting_info(self: Pin<&Self>, _window: &ComponentWindow) -> LayoutInfo {
        LayoutInfo::default()
    }

    fn input_event(
        self: Pin<&Self>,
        _event: MouseEvent,
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

impl ItemConsts for Window {
    const cached_rendering_data_offset: const_field_offset::FieldOffset<Self, CachedRenderingData> =
        Self::FIELD_OFFSETS.cached_rendering_data.as_unpinned_projection();
}

ItemVTable_static! {
    /// The VTable for `Window`
    #[no_mangle]
    pub static WindowVTable for Window
}

ItemVTable_static! {
    /// The VTable for `Text`
    #[no_mangle]
    pub static TextVTable for Text
}

ItemVTable_static! {
    /// The VTable for `TextInput`
    #[no_mangle]
    pub static TextInputVTable for TextInput
}
