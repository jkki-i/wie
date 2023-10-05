use alloc::rc::Rc;
use core::{fmt::Debug, num::NonZeroU32};

use softbuffer::{Context, Surface};
use tao::{
    dpi::PhysicalSize,
    event::{ElementState, Event, KeyEvent, WindowEvent},
    event_loop::{ControlFlow, EventLoop, EventLoopBuilder, EventLoopProxy},
    keyboard::KeyCode,
    window::{Window as TaoWindow, WindowBuilder},
};

use wie_backend::{canvas::Canvas, Window};

#[derive(Debug)]
pub enum WindowInternalEvent {
    RequestRedraw,
    Paint(Vec<u32>),
}

pub enum WindowCallbackEvent {
    Update,
    Redraw,
    Keydown(KeyCode),
    Keyup(KeyCode),
}

pub struct WindowProxy {
    window: Rc<TaoWindow>,
    event_loop_proxy: EventLoopProxy<WindowInternalEvent>,
}

impl WindowProxy {
    fn send_event(&self, event: WindowInternalEvent) -> anyhow::Result<()> {
        self.event_loop_proxy.send_event(event)?;

        Ok(())
    }
}

impl Window for WindowProxy {
    fn request_redraw(&self) -> anyhow::Result<()> {
        self.send_event(WindowInternalEvent::RequestRedraw)
    }

    fn repaint(&self, canvas: &dyn Canvas) -> anyhow::Result<()> {
        let data = canvas
            .colors()
            .iter()
            .map(|x| ((x.a as u32) << 24) | ((x.r as u32) << 16) | ((x.g as u32) << 8) | (x.b as u32))
            .collect::<Vec<_>>();

        self.send_event(WindowInternalEvent::Paint(data))
    }

    fn width(&self) -> u32 {
        self.window.inner_size().width
    }

    fn height(&self) -> u32 {
        self.window.inner_size().height
    }
}

pub struct WindowImpl {
    window: Rc<TaoWindow>,
    event_loop: EventLoop<WindowInternalEvent>,
    surface: Surface,
}

impl WindowImpl {
    pub fn new(width: u32, height: u32) -> anyhow::Result<Self> {
        let event_loop = EventLoopBuilder::<WindowInternalEvent>::with_user_event().build();

        let size = PhysicalSize::new(width, height);

        let builder = WindowBuilder::new().with_inner_size(size).with_title("WIPI");

        let window = builder.build(&event_loop)?;
        // TODO we need to render to gtk window instead of whole wayland window to make decoration work on linux
        let context = unsafe { Context::new(&window) }.map_err(|x| anyhow::anyhow!("{:?}", x))?;
        let mut surface = unsafe { Surface::new(&context, &window) }.map_err(|x| anyhow::anyhow!("{:?}", x))?;

        surface
            .resize(NonZeroU32::new(size.width).unwrap(), NonZeroU32::new(size.height).unwrap())
            .map_err(|x| anyhow::anyhow!("{:?}", x))?;

        Ok(Self {
            window: Rc::new(window),
            event_loop,
            surface,
        })
    }

    pub fn proxy(&self) -> WindowProxy {
        WindowProxy {
            window: self.window.clone(),
            event_loop_proxy: self.event_loop.create_proxy(),
        }
    }

    fn callback<C, E>(event: WindowCallbackEvent, control_flow: &mut ControlFlow, callback: &mut C)
    where
        C: FnMut(WindowCallbackEvent) -> Result<(), E> + 'static,
        E: Debug,
    {
        let result = callback(event);
        if let Err(x) = result {
            tracing::error!(target: "wie", "{:?}", x);

            *control_flow = ControlFlow::Exit;
        }
    }

    pub fn run<C, E>(mut self, mut callback: C) -> !
    where
        C: FnMut(WindowCallbackEvent) -> Result<(), E> + 'static,
        E: Debug,
    {
        self.event_loop.run(move |event, _, control_flow| match event {
            Event::UserEvent(x) => match x {
                WindowInternalEvent::RequestRedraw => {
                    self.window.request_redraw();
                }
                WindowInternalEvent::Paint(data) => {
                    let mut buffer = self.surface.buffer_mut().unwrap();
                    buffer.copy_from_slice(&data);

                    buffer.present().unwrap();
                }
            },

            Event::WindowEvent { event, .. } => match event {
                WindowEvent::CloseRequested => {
                    *control_flow = ControlFlow::Exit;
                }
                WindowEvent::KeyboardInput {
                    event:
                        KeyEvent {
                            physical_key,
                            state: ElementState::Pressed,
                            ..
                        },
                    ..
                } => {
                    Self::callback(WindowCallbackEvent::Keydown(physical_key), control_flow, &mut callback);
                }
                WindowEvent::KeyboardInput {
                    event:
                        KeyEvent {
                            physical_key,
                            state: ElementState::Released,
                            ..
                        },
                    ..
                } => {
                    Self::callback(WindowCallbackEvent::Keyup(physical_key), control_flow, &mut callback);
                }
                _ => {}
            },
            Event::MainEventsCleared => {
                Self::callback(WindowCallbackEvent::Update, control_flow, &mut callback);
            }
            Event::RedrawRequested(_) => {
                Self::callback(WindowCallbackEvent::Redraw, control_flow, &mut callback);
            }

            _ => {}
        })
    }
}