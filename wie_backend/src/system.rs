mod audio;
pub mod canvas;
pub mod database;
mod resource;
pub mod time;
pub mod window;

use alloc::{collections::VecDeque, rc::Rc};
use core::cell::{Ref, RefCell, RefMut};

use wie_base::Event;

use crate::{extract_zip, platform::Platform};

use self::{
    audio::Audio,
    canvas::{ArgbPixel, Canvas, ImageBuffer},
    database::DatabaseRepository,
    resource::Resource,
    time::Time,
    window::Window,
};

#[derive(Clone)]
pub struct System {
    database: Rc<RefCell<DatabaseRepository>>,
    resource: Rc<RefCell<Resource>>,
    time: Rc<RefCell<Time>>,
    screen_canvas: Rc<RefCell<Box<dyn Canvas>>>,
    events: Rc<RefCell<VecDeque<Event>>>,
    window: Rc<RefCell<Box<dyn Window>>>,
    audio: Rc<RefCell<Audio>>,
}

impl System {
    pub fn new(app_id: &str, platform: &mut dyn Platform) -> Self {
        let window = platform.create_window();
        let screen_canvas = ImageBuffer::<ArgbPixel>::new(window.width(), window.height());

        Self {
            database: Rc::new(RefCell::new(DatabaseRepository::new(app_id))),
            resource: Rc::new(RefCell::new(Resource::new())),
            time: Rc::new(RefCell::new(Time::new())),
            screen_canvas: Rc::new(RefCell::new(Box::new(screen_canvas))),
            events: Rc::new(RefCell::new(VecDeque::new())),
            window: Rc::new(RefCell::new(window)),
            audio: Rc::new(RefCell::new(Audio::new())),
        }
    }

    pub fn database(&self) -> RefMut<'_, DatabaseRepository> {
        (*self.database).borrow_mut()
    }

    pub fn resource(&self) -> Ref<'_, Resource> {
        (*self.resource).borrow()
    }

    pub fn time(&self) -> Ref<'_, Time> {
        (*self.time).borrow()
    }

    pub fn screen_canvas(&self) -> RefMut<'_, Box<dyn Canvas>> {
        (*self.screen_canvas).borrow_mut()
    }

    pub fn window(&self) -> RefMut<'_, Box<dyn Window>> {
        (*self.window).borrow_mut()
    }

    pub fn audio(&self) -> RefMut<'_, Audio> {
        (*self.audio).borrow_mut()
    }

    pub fn pop_event(&self) -> Option<Event> {
        (*self.events).borrow_mut().pop_front()
    }

    pub fn push_event(&self, event: Event) {
        (*self.events).borrow_mut().push_back(event);
    }

    pub fn repaint(&self) -> anyhow::Result<()> {
        self.window().repaint(&**self.screen_canvas())
    }

    pub fn mount_zip(&self, zip: &[u8]) -> anyhow::Result<()> {
        let files = extract_zip(zip)?;

        let mut resource = (*self.resource).borrow_mut();
        for (path, data) in files {
            resource.add(&path, data);
        }

        Ok(())
    }
}