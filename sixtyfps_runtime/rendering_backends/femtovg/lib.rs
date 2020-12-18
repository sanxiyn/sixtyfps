use sixtyfps_corelib::eventloop::ComponentWindow;
use sixtyfps_corelib::graphics::{
    Color, Frame as GraphicsFrame, GraphicsBackend, GraphicsWindow, HighLevelRenderingPrimitive,
    IntRect, Point, Rect, RenderingPrimitivesBuilder, RenderingVariables, Resource, RgbaColor,
    Size,
};

type Canvas = femtovg::Canvas<femtovg::renderer::OpenGl>;

enum RenderingPrimitive {
    Primitive(HighLevelRenderingPrimitive),
    Image { image: femtovg::ImageId, source_clip_rect: IntRect },
    RestoreState,
}

struct CanvasFrame {
    canvas: Canvas,
    #[cfg(not(target_arch = "wasm32"))]
    windowed_context: glutin::WindowedContext<glutin::PossiblyCurrent>,
}

impl GraphicsFrame for CanvasFrame {
    type LowLevelRenderingPrimitive = RenderingPrimitive;

    fn render_primitive(
        &mut self,
        primitive: &Self::LowLevelRenderingPrimitive,
        translation: Point,
        variables: RenderingVariables,
    ) -> Vec<Self::LowLevelRenderingPrimitive> {
        if matches!(primitive, RenderingPrimitive::RestoreState) {
            self.canvas.restore();
            return vec![];
        }
        if let RenderingPrimitive::Primitive(HighLevelRenderingPrimitive::ClipRect {
            width,
            height,
        }) = primitive
        {
            self.canvas.scissor(0., 0., *width, *height);
            return vec![RenderingPrimitive::RestoreState];
        }

        self.canvas.save();
        self.canvas.translate(translation.x, translation.y);

        match (&primitive, &variables) {
            (RenderingPrimitive::Primitive(HighLevelRenderingPrimitive::NoContents), _) => {}
            (
                RenderingPrimitive::Primitive(HighLevelRenderingPrimitive::Rectangle {
                    width,
                    height,
                }),
                RenderingVariables::Rectangle { fill, stroke, border_width, border_radius },
            ) => {
                let mut path = femtovg::Path::new();
                path.rounded_rect(0., 0., *width, *height, *border_radius);

                let fill_paint = femtovg::Paint::color(fill.into());
                self.canvas.fill_path(&mut path, fill_paint);
                let mut stroke_paint = femtovg::Paint::color(stroke.into());
                stroke_paint.set_line_width(*border_width);
                self.canvas.stroke_path(&mut path, stroke_paint);
            }
            (
                RenderingPrimitive::Image { image, source_clip_rect },
                RenderingVariables::Image { scaled_width, scaled_height, fit },
            ) => {
                let info = self.canvas.image_info(*image).unwrap();
                let (image_width, image_height) = (info.width() as f32, info.height() as f32);
                let (source_width, source_height) = if source_clip_rect.is_empty() {
                    (image_width, image_height)
                } else {
                    (source_clip_rect.width() as _, source_clip_rect.height() as _)
                };
                let fill_paint = femtovg::Paint::image(
                    *image,
                    source_clip_rect.min_x() as _,
                    source_clip_rect.min_y() as _,
                    source_width,
                    source_height,
                    0.0,
                    1.0,
                );

                let mut path = femtovg::Path::new();
                path.rect(0., 0., image_width, image_height);

                if *scaled_width > 0. && *scaled_width > 0. {
                    self.canvas.scale(*scaled_width / image_width, *scaled_height / image_height);
                }

                self.canvas.fill_path(&mut path, fill_paint);
            }
            (
                RenderingPrimitive::Primitive(HighLevelRenderingPrimitive::Text {
                    text,
                    font_request,
                }),
                RenderingVariables::Text { translate, color, cursor, selection },
            ) => {}
            (
                RenderingPrimitive::Primitive(HighLevelRenderingPrimitive::Path {
                    width,
                    height,
                    elements,
                    stroke_width,
                }),
                RenderingVariables::Path { fill, stroke },
            ) => {}
            (
                RenderingPrimitive::Primitive(HighLevelRenderingPrimitive::ClipRect {
                    width,
                    height,
                }),
                _,
            ) => {
                unreachable!()
            }
            (RenderingPrimitive::Primitive(HighLevelRenderingPrimitive::Rectangle { .. }), _) => {
                unreachable!()
            }
            (
                RenderingPrimitive::Primitive(HighLevelRenderingPrimitive::Image {
                    source,
                    source_clip_rect,
                }),
                _,
            ) => {
                unreachable!()
            }
            (
                RenderingPrimitive::Primitive(HighLevelRenderingPrimitive::Text {
                    text,
                    font_request,
                }),
                _,
            ) => {
                unreachable!()
            }
            (
                RenderingPrimitive::Primitive(HighLevelRenderingPrimitive::Path {
                    width,
                    height,
                    elements,
                    stroke_width,
                }),
                _,
            ) => {
                unreachable!()
            }
            (RenderingPrimitive::Image { .. }, _) => {
                unreachable!()
            }
            (RenderingPrimitive::RestoreState, _) => {
                unreachable!()
            }
        };
        self.canvas.restore();
        vec![]
    }
}

struct CanvasBuilder {
    canvas: Canvas,
    #[cfg(not(target_arch = "wasm32"))]
    windowed_context: glutin::WindowedContext<glutin::PossiblyCurrent>,
}

impl RenderingPrimitivesBuilder for CanvasBuilder {
    type LowLevelRenderingPrimitive = RenderingPrimitive;

    fn create(
        &mut self,
        primitive: HighLevelRenderingPrimitive,
    ) -> Self::LowLevelRenderingPrimitive {
        match primitive {
            HighLevelRenderingPrimitive::Image { source, source_clip_rect } => match source {
                Resource::None => {
                    RenderingPrimitive::Primitive(HighLevelRenderingPrimitive::NoContents)
                }
                Resource::AbsoluteFilePath(path) => RenderingPrimitive::Image {
                    image: self
                        .canvas
                        .load_image_file(
                            std::path::Path::new(&path.as_str()),
                            femtovg::ImageFlags::empty(),
                        )
                        .unwrap(),
                    source_clip_rect,
                },
                Resource::EmbeddedData(data) => RenderingPrimitive::Image {
                    image: self
                        .canvas
                        .load_image_mem(data.as_slice(), femtovg::ImageFlags::empty())
                        .unwrap(),
                    source_clip_rect,
                },
                Resource::EmbeddedRgbaImage { width, height, data } => todo!(),
            },
            primitive @ _ => RenderingPrimitive::Primitive(primitive),
        }
    }
}

struct Renderer {
    canvas: Option<Canvas>,
    #[cfg(not(target_arch = "wasm32"))]
    windowed_context: Option<glutin::WindowedContext<glutin::NotCurrent>>,
}

impl GraphicsBackend for Renderer {
    type LowLevelRenderingPrimitive = RenderingPrimitive;

    type Frame = CanvasFrame;

    type RenderingPrimitivesBuilder = CanvasBuilder;

    fn new_rendering_primitives_builder(&mut self) -> Self::RenderingPrimitivesBuilder {
        #[cfg(not(target_arch = "wasm32"))]
        let current_windowed_context =
            unsafe { self.windowed_context.take().unwrap().make_current().unwrap() };

        CanvasBuilder {
            canvas: self.canvas.take().unwrap(),
            #[cfg(not(target_arch = "wasm32"))]
            windowed_context: current_windowed_context,
        }
    }

    fn finish_primitives(&mut self, mut builder: Self::RenderingPrimitivesBuilder) {
        builder.canvas.flush();

        #[cfg(not(target_arch = "wasm32"))]
        {
            self.windowed_context =
                Some(unsafe { builder.windowed_context.make_not_current().unwrap() });
        }

        self.canvas = Some(builder.canvas)
    }

    fn new_frame(&mut self, width: u32, height: u32, clear_color: &Color) -> Self::Frame {
        let mut canvas = self.canvas.take().unwrap();

        #[cfg(not(target_arch = "wasm32"))]
        let current_windowed_context =
            unsafe { self.windowed_context.take().unwrap().make_current().unwrap() };

        {
            let dpi_factor = current_windowed_context.window().scale_factor();
            canvas.set_size(width as u32, height as u32, dpi_factor as f32);
        }

        canvas.clear_rect(0, 0, width, height, clear_color.into());

        CanvasFrame { canvas, windowed_context: current_windowed_context }
    }

    fn present_frame(&mut self, frame: Self::Frame) {
        let mut canvas = frame.canvas;
        canvas.flush();

        #[cfg(not(target_arch = "wasm32"))]
        {
            frame.windowed_context.swap_buffers().unwrap();

            self.windowed_context =
                Some(unsafe { frame.windowed_context.make_not_current().unwrap() });
        }

        self.canvas = Some(canvas);
    }

    fn window(&self) -> &glutin::window::Window {
        #[cfg(not(target_arch = "wasm32"))]
        return self.windowed_context.as_ref().unwrap().window();
    }
}

impl Renderer {
    pub fn new(
        event_loop: &winit::event_loop::EventLoop<sixtyfps_corelib::eventloop::CustomEvent>,
        window_builder: winit::window::WindowBuilder,
        #[cfg(target_arch = "wasm32")] canvas_id: &str,
    ) -> Renderer {
        #[cfg(not(target_arch = "wasm32"))]
        let (windowed_context, canvas_renderer) = {
            let windowed_context = glutin::ContextBuilder::new()
                .with_vsync(true)
                .build_windowed(window_builder, &event_loop)
                .unwrap();
            let windowed_context = unsafe { windowed_context.make_current().unwrap() };

            let canvas_renderer = femtovg::renderer::OpenGl::new(|s| {
                windowed_context.get_proc_address(s) as *const _
            })
            .unwrap();

            #[cfg(target_os = "macos")]
            {
                use cocoa::appkit::NSView;
                use winit::platform::macos::WindowExtMacOS;
                let ns_view = windowed_context.window().ns_view();
                let view_id: cocoa::base::id = ns_view as *const _ as *mut _;
                unsafe {
                    NSView::setLayerContentsPlacement(view_id, cocoa::appkit::NSViewLayerContentsPlacement::NSViewLayerContentsPlacementTopLeft)
                }
            }

            (windowed_context, canvas_renderer)
        };

        let canvas = femtovg::Canvas::new(canvas_renderer).unwrap();

        Self {
            canvas: Some(canvas),
            windowed_context: Some(unsafe { windowed_context.make_not_current().unwrap() }),
        }
    }
}

pub fn create_gl_window() -> ComponentWindow {
    ComponentWindow::new(GraphicsWindow::new(|event_loop, window_builder| {
        Renderer::new(
            &event_loop.get_winit_event_loop(),
            window_builder,
            #[cfg(target_arch = "wasm32")]
            "canvas",
        )
    }))
}
