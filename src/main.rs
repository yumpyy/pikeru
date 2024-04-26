use img::load_from_memory;
use num_cpus;
use itertools::Itertools;
mod wrapper;
mod iced_drop;
use iced::{
    advanced::widget::Id,
    Rectangle,
    color, Background, alignment, executor, subscription,
    Application, Command, Length, Element, theme::Container,
    mouse::Event::{ButtonPressed, WheelScrolled},
    mouse::Button::{Back,Forward},
    mouse::ScrollDelta,
    keyboard::Event::{KeyPressed,KeyReleased},
    keyboard::Key,
    keyboard::key::Named::{Shift,Control},
    widget::{
        vertical_space,
        container::{Appearance, StyleSheet,Id as CId},
        image, image::Handle, Column, Row, text, responsive,
        Scrollable, scrollable, scrollable::{Direction,Properties},
        Button, TextInput,
        column, row, mouse_area, container,
    },
    futures::{
        channel::mpsc,
        sink::SinkExt,
        StreamExt,
    },
    event::{self, Event::{Mouse,Keyboard}},
    Point,
};
use tokio::{
    fs::File, io::AsyncReadExt,
};
use std::{
    path::{PathBuf,Path},
    mem,
    process,
    sync::Arc,
    time::{Instant,Duration},
};

const THUMBSIZE: f32 = 160.0;

fn main() -> iced::Result {
    FilePicker::run(iced::Settings::default())
}

#[derive(Debug, Clone)]
enum Message {
    LoadDir,
    LoadBookmark(usize),
    Open,
    Cancel,
    UpDir,
    Init(mpsc::Sender<Item>),
    NextItem(Item),
    LeftClick(usize),
    MiddleClick(usize),
    RightClick(i64),
    TxtInput(String),
    Shift(bool),
    Ctrl(bool),
    Drop(usize, Point),
    HandleZones(usize, Vec<(Id, iced::Rectangle)>),
    NextImage(i64),
    Scrolled(scrollable::Viewport),
    PositionInfo(Point, Rectangle),
}

enum SubState {
    Starting,
    Ready(mpsc::Receiver<Item>),
}

#[derive(Debug, Clone, Default, PartialEq)]
enum FType {
    File,
    Image,
    Dir,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Default)]
struct Item {
    path: String,
    ftype: FType,
    handle: Option<Handle>,
    idx: usize,
    sel: bool,
    nav_id: u8,
    mtime: f32,
}

struct Icons {
    folder: Handle,
    doc: Handle,
    unknown: Handle,
    error: Handle,
}

struct Bookmark {
    label: String,
    path: String,
    id: CId,
}

struct FilePicker {
    scroll_id: scrollable::Id,
    items: Vec<Item>,
    dirs: Vec<String>,
    bookmarks: Vec<Bookmark>,
    inputbar: String,
    thumb_sender: Option<mpsc::Sender<Item>>,
    nproc: usize,
    last_loaded: usize,
    last_clicked: Option<usize>,
    icons: Arc<Icons>,
    clicktimer: ClickTimer,
    ctrl_pressed: bool,
    shift_pressed: bool,
    nav_id: u8,
    show_hidden: bool,
    view_image: (usize, Option<Handle>),
    scroll_offset: scrollable::RelativeOffset,
}

impl Application for FilePicker {
    type Executor = executor::Default;
    type Message = Message;
    type Theme = iced::Theme;
    type Flags = ();

    fn new(_flags: Self::Flags) -> (Self, iced::Command<Self::Message>) {
        (
            Self {
                items: Default::default(),
                thumb_sender: None,
                nproc: num_cpus::get(),
                dirs: vec![
                    "/home/d/sync/docs/pics".into(),
                ],
                bookmarks: vec![
                    Bookmark::new("Home", "/home/d"),
                    Bookmark::new("Pictures", "/home/d/Pictures"),
                    Bookmark::new("Documents", "/home/d/Documents"),
                ],
                last_loaded: 0,
                last_clicked: None,
                inputbar: Default::default(),
                icons: Arc::new(Icons::new()),
                clicktimer: ClickTimer{ idx:0, time: Instant::now() - Duration::from_secs(1)},
                ctrl_pressed: false,
                shift_pressed: false,
                scroll_id: scrollable::Id::unique(),
                nav_id: 0,
                show_hidden: false,
                view_image: (0, None),
                scroll_offset: scrollable::RelativeOffset::START,
            },
            Command::none(),
        )
    }

    fn theme(&self) -> iced::Theme {
        iced::Theme::Dark
    }

    fn title(&self) -> String {
        String::from("File Picker")
    }

    fn update(&mut self, message: Self::Message) -> iced::Command<Self::Message> {
        match message {
            Message::PositionInfo(p, r) => {
                eprintln!("{:?} {:?}", p, r);
            },
            Message::Drop(idx, cursor_pos) => {
                return iced_drop::zones_on_point(
                    move |zones| Message::HandleZones(idx, zones),
                    cursor_pos, None, None,
                );
            }
            Message::HandleZones(idx, zones) => {
                if zones.len() > 0 {
                    let targets: Vec<_> = self.bookmarks.iter().enumerate().filter_map(|(i, bm)| {
                        if zones[0].0 == bm.id.clone().into() {
                            Some(i)
                        } else {None}
                    }).collect();
                    let target = if targets.len() > 0 {
                        Some(targets[0] as i32)
                    } else if zones[0].0 == Id::new("bookmarks") {
                        Some(-1)
                    } else { None };
                    self.add_bookmark(idx, target);
                }
            }
            Message::Init(chan) => {
                self.thumb_sender = Some(chan);
                return self.update(Message::LoadDir);
            },
            Message::Scrolled(viewport) => self.scroll_offset = viewport.relative_offset(),
            Message::TxtInput(txt) => self.inputbar = txt,
            Message::Ctrl(pressed) => self.ctrl_pressed = pressed,
            Message::Shift(pressed) => self.shift_pressed = pressed,
            Message::NextItem(doneitem) => {
                if doneitem.nav_id == self.nav_id {
                    self.last_loaded += 1;
                    if self.last_loaded < self.items.len() {
                        let nextitem = mem::take(&mut self.items[self.last_loaded]);
                        tokio::task::spawn(nextitem.load(self.thumb_sender.as_ref().unwrap().clone(), self.icons.clone()));
                    }
                    let i = doneitem.idx;
                    self.items[i] = doneitem;
                }
            },
            Message::LoadBookmark(idx) => {
                self.dirs = vec![self.bookmarks[idx].path.clone()];
                return self.update(Message::LoadDir);
            },
            Message::LoadDir => {
                self.inputbar = self.dirs[0].clone();
                self.load_dir();
                self.last_loaded = self.nproc.min(self.items.len());
                for i in 0..self.last_loaded {
                    let item = mem::take(&mut self.items[i]);
                    tokio::task::spawn(item.load(self.thumb_sender.as_ref().unwrap().clone(), self.icons.clone()));
                }
                return scrollable::snap_to(self.scroll_id.clone(), scrollable::RelativeOffset::START);
            },
            Message::UpDir => {
                self.dirs = self.dirs.iter().map(|dir| Path::new(dir.as_str()).parent().unwrap()
                                                 .as_os_str().to_str().unwrap().to_string())
                    .unique_by(|s|s.to_owned()).collect();
                return self.update(Message::LoadDir);
            },
            Message::MiddleClick(idx) => self.click_item(idx, false, true),
            Message::LeftClick(idx) => {
                match self.clicktimer.click(idx) {
                    ClickType::Single => self.click_item(idx, self.shift_pressed, self.ctrl_pressed),
                    ClickType::Double => return self.update(Message::Open),
                }
            },
            Message::RightClick(idx) => {
                if idx >= 0 {
                    let item = &self.items[idx as usize];
                    if item.ftype == FType::Image {
                        self.view_image = (idx as usize, Some(Handle::from_path(item.path.as_str())));
                    } else {
                        self.click_item(idx as usize, true, false);
                    }
                } else {
                    self.view_image = (0, None);
                    return scrollable::snap_to(self.scroll_id.clone(), self.scroll_offset);
                }
            },
            Message::NextImage(y) => {
                if self.view_image.1 != None {
                    let mut i = self.view_image.0;
                    while (y<0 && i>0) || (y>0 && i<self.items.len()-1) {
                        i = ((i as i64) + y) as usize;
                        if self.items[i as usize].ftype == FType::Image {
                            self.view_image = (i as usize, Some(Handle::from_path(self.items[i].path.as_str())));
                            return self.update(Message::LeftClick(i as usize));
                        }
                    }
                }
            }
            Message::Open => {
                let sels: Vec<&Item> = self.items.iter().filter(|item| item.sel ).collect();
                if sels.len() != 0 {
                    match sels[0].ftype {
                        FType::Dir => {
                            self.dirs = sels.iter().filter_map(|item| match item.ftype {
                                FType::Dir => Some(item.path.clone()), _ => None}).collect();
                            return self.update(Message::LoadDir);
                        },
                        _ => {
                            sels.iter().for_each(|item| println!("{}", item.path));
                            process::exit(0);
                        }
                    }
                }
            },
            Message::Cancel => process::exit(0),
        }
        Command::none()
    }

    fn subscription(&self) -> iced::Subscription<Self::Message> {
        let mut state = SubState::Starting;
        let items = subscription::channel("", 100, |mut messager| async move {
            loop {
                match &mut state {
                    SubState::Starting => {
                        let (sender, receiver) = mpsc::channel(100);
                        messager.send(Message::Init(sender)).await.unwrap();
                        state = SubState::Ready(receiver);
                    }
                    SubState::Ready(thumb_receiver) => {
                        let item = thumb_receiver.select_next_some().await;
                        messager.send(Message::NextItem(item)).await.unwrap();
                    },
                }
            }
        });
        let events = event::listen_with(|evt, _| {
            match evt {
                Mouse(ButtonPressed(Back)) => Some(Message::UpDir),
                Mouse(ButtonPressed(Forward)) => None,
                Mouse(WheelScrolled{ delta: ScrollDelta::Lines{ y, ..}}) => Some(Message::NextImage(if y<0.0 {1} else {-1})),
                Keyboard(KeyPressed{ key: Key::Named(Shift), .. }) => Some(Message::Shift(true)),
                Keyboard(KeyReleased{ key: Key::Named(Shift), .. }) => Some(Message::Shift(false)),
                Keyboard(KeyPressed{ key: Key::Named(Control), .. }) => Some(Message::Ctrl(true)),
                Keyboard(KeyReleased{ key: Key::Named(Control), .. }) => Some(Message::Ctrl(false)),
                _ => None,
            }
        });
        subscription::Subscription::batch(vec![items, events/*, native*/])
    }

    fn view(&self) -> iced::Element<'_, Self::Message> {
        responsive(|size| {
            let ctrlbar = column![
                row![
                    top_button("Cmd", 80.0, Message::Cancel),
                    top_button("View", 80.0, Message::Cancel),
                    top_button("New Dir", 80.0, Message::Cancel),
                    top_button("Up Dir", 80.0, Message::UpDir),
                    top_button("Cancel", 100.0, Message::Cancel),
                    top_button("Open", 100.0, Message::Open)
                ].spacing(2),
                TextInput::new("directory or file path", self.inputbar.as_str())
                    .on_input(Message::TxtInput)
                    .on_paste(Message::TxtInput),
            ].align_items(iced::Alignment::End).width(Length::Fill);
            let bookmarks = self.bookmarks.iter().enumerate().fold(column![], |col,(i,bm)| {
                        col.push(Button::new(
                                    container(
                                        text(bm.label.as_str())
                                           .horizontal_alignment(alignment::Horizontal::Center)
                                           .width(Length::Fill)).id(bm.id.clone()))
                                     .on_press(Message::LoadBookmark(i)))
                    }).push(container(vertical_space()).height(Length::Fill).width(Length::Fill)
                            .id(CId::new("bookmarks"))).width(Length::Fixed(120.0));

            let content: iced::Element<'_, Self::Message> = if let Some(handle) = &self.view_image.1 {
                mouse_area(container(image(handle.clone())
                                    .width(Length::Fill)
                                    .height(Length::Fill))
                               .align_x(alignment::Horizontal::Center)
                               .align_y(alignment::Vertical::Center)
                               .width(Length::Fill).height(Length::Fill)
                    ).on_right_press(Message::RightClick(-1))
                    .into()
            } else {
                let maxcols = ((size.width-130.0) / THUMBSIZE).max(1.0) as usize;
                let num_rows = self.items.len() / maxcols + if self.items.len() % maxcols != 0 { 1 } else { 0 };
                let mut rows = Column::new();
                for i in 0..num_rows {
                    let start = i * maxcols;
                    let mut row = Row::new().width(Length::Fill);
                    for j in 0..maxcols {
                        let idx = start + j;
                        if idx < self.items.len() {
                            row = row.push(unsafe{self.items.get_unchecked(idx)}.display());
                        }
                    }
                    rows = rows.push(row);
                }
                Scrollable::new(rows)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .on_scroll(Message::Scrolled)
                    .direction(Direction::Vertical(Properties::new()))
                    .id(self.scroll_id.clone()).into()
            };
            column![
                ctrlbar,
                row![bookmarks, content],
            ].into()
        }).into()
    }
}

fn top_button(txt: &str, size: f32, msg: Message) -> Element<'static, Message> {
    Button::new(text(txt)
                .width(size)
                .horizontal_alignment(alignment::Horizontal::Center))
        .on_press(msg).into()
}

impl Item {

    fn display(&self) -> Element<'static, Message> {
        let mut col = Column::new()
            .align_items(iced::Alignment::Center)
            .width(THUMBSIZE);
        if let Some(h) = &self.handle {
            col = col.push(image(h.clone()));
        }
        let mut label = self.path.rsplitn(2,'/').next().unwrap();
        col = if label.len() > 16 {
            label = &label[(label.len()-16)..label.len()];
            let mut shortened = ['.' as u8; 19];
            shortened[3..3+label.len()].copy_from_slice(label.as_bytes());
            col.push(text(unsafe{std::str::from_utf8_unchecked(&shortened)})).into()
        } else {
            col.push(text(label)).into()
        };
        let clickable = if self.isdir() {
            let idx = self.idx;
            let dr = iced_drop::droppable(col).on_drop(move |point,_| Message::Drop(idx, point));
            if self.sel {
                mouse_area(container(dr).style(get_sel_theme()))
            } else {
                mouse_area(dr)
            }
        } else {
            if self.sel {
                mouse_area(container(col).style(get_sel_theme()))
            } else {
                mouse_area(col)
            }
        }.on_release(Message::LeftClick(self.idx))
            .on_right_press(Message::RightClick(self.idx as i64))
            .on_middle_press(Message::MiddleClick(self.idx));
        clickable.into()
    }

    fn isdir(self: &Self) -> bool {
        return self.ftype == FType::Dir;
    }

    fn new(pth: PathBuf, nav_id: u8) -> Self {
        let md = pth.metadata().unwrap();
        let ftype = if md.is_dir() {
            FType::Dir
        } else {
            FType::Unknown
        };
        let mtime = md.modified().unwrap();
        Item {
            path: pth.to_string_lossy().to_string(),
            ftype,
            idx: 0,
            handle: None,
            sel: false,
            nav_id,
            mtime: mtime.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs_f32(),
        }
    }

    async fn load(mut self, mut chan: mpsc::Sender<Item>, icons: Arc<Icons>) {
        match self.ftype {
            FType::Dir => {
                self.handle = Some(icons.folder.clone());
            },
            _ => {
                let ext = match self.path.rsplitn(2,'.').next() {
                    Some(s) => s,
                    None => "",
                };
                self.ftype = match ext.to_lowercase().as_str() {
                    "png"|"jpg"|"jpeg"|"bmp"|"tiff"|"gif"|"webp" => {
                        let file = File::open(self.path.as_str()).await;
                        match file {
                            Ok(mut file) => {
                                let mut buffer = Vec::new();
                                file.read_to_end(&mut buffer).await.unwrap_or(0);
                                let img = load_from_memory(buffer.as_ref());
                                match img {
                                    Ok(img) => {
                                        let thumb = img.thumbnail(THUMBSIZE as u32, THUMBSIZE as u32);
                                        let (w,h,rgba) = (thumb.width(), thumb.height(), thumb.into_rgba8());
                                        self.handle = Some(Handle::from_pixels(w, h, rgba.as_raw().clone()));
                                        FType::Image
                                    },
                                    Err(e) => {
                                        eprintln!("Error loading image {}: {}", self.path, e);
                                        self.handle = Some(icons.error.clone());
                                        FType::File
                                    },
                                }
                            },
                            Err(e) => {
                                eprintln!("Error reading {}: {}", self.path, e);
                                self.handle = Some(icons.error.clone());
                                FType::File
                            },
                        }
                    },
                    "txt"|"pdf"|"doc"|"docx"|"xls"|"xlsx" => {
                        self.handle = Some(icons.doc.clone());
                        FType::File
                    },
                    _ => {
                        self.handle = Some(icons.unknown.clone());
                        FType::File
                    },
                };
            }
        }
        chan.send(self).await.unwrap();
    }
}

impl FilePicker {
    fn click_item(self: &mut Self, i: usize, shift: bool, ctrl: bool) {
        self.last_clicked = Some(i);
        let isdir = self.items[i].isdir();
        let prevsel = self.items.iter().filter_map(|item| if item.sel { Some(item.idx) } else { None }).collect::<Vec<usize>>();
        while shift && prevsel.len() > 0 {
            let prevdir = self.items[prevsel[0]].isdir();
            if prevdir != isdir {
                break;
            }
            let mut lo = self.items[i].idx;
            let mut hi = lo;
            prevsel.iter().for_each(|j| {
                lo = lo.min(self.items[*j].idx);
                hi = hi.max(self.items[*j].idx);
            });
            for j in lo..=hi {
                self.items[j].sel = self.items[j].isdir() == isdir;
            }
            return;
        }
        if !self.items[i].sel {
            self.items[i].sel = true;
        } else if prevsel.len() == 1 || ctrl {
            self.items[i].sel = false;
        }
        prevsel.iter().for_each(|j| {
            if !ctrl || self.items[*j].isdir() != isdir { self.items[*j].sel = false; }
        });
        if self.items[i].sel {
            self.inputbar = self.items[i].path.clone();
        } else {
            self.inputbar = self.dirs[0].clone();
        }
    }

    fn load_dir(self: &mut Self) {
        let mut ret = vec![];
        self.nav_id = self.nav_id.wrapping_add(1);
        for dir in self.dirs.iter() {
            let entries: Vec<_> = std::fs::read_dir(dir.as_str()).unwrap().map(|f| f.unwrap().path()).collect();
            entries.iter().filter(|path|{ self.show_hidden ||
                !path.as_os_str().to_str().map(|s|s.rsplitn(2,'/').next().unwrap().starts_with('.')).unwrap_or(false)
            }).for_each(|path| {
                ret.push(Item::new(path.into(), self.nav_id));
            });
        }
        ret.sort_unstable_by(|a,b| {
            b.isdir().cmp(&a.isdir()).then_with(||a.path.cmp(&b.path))
        });
        ret.iter_mut().enumerate().for_each(|(i, item)| item.idx = i);
        self.items = ret
    }

    fn add_bookmark(self: &mut Self, dragged: usize, target: Option<i32>) {
        let item = &self.items[dragged];
        let label = item.path.rsplitn(2,'/').next().unwrap();
        match target {
            Some(i) if i >= 0 => {
                // TODO: multi-dir bookmark?
                self.bookmarks.push(Bookmark::new(label, item.path.as_str()));
            },
            Some(_) => {
                self.bookmarks.push(Bookmark::new(label, item.path.as_str()));
            },
            None => {},
        }
    }
}

impl Icons {
    fn new() -> Self {
        Self {
            folder: Self::init(include_bytes!("../assets/folder.png")),
            unknown:  Self::init(include_bytes!("../assets/unknown.png")),
            doc:  Self::init(include_bytes!("../assets/document.png")),
            error:  Self::init(include_bytes!("../assets/error.png")),
        }
    }
    fn init(img_bytes: &[u8]) -> Handle {
        let img = load_from_memory(img_bytes).unwrap();
        let thumb = img.thumbnail((THUMBSIZE * 0.9) as u32, (THUMBSIZE * 0.9) as u32);
        let (w,h,rgba) = (thumb.width(), thumb.height(), thumb.into_rgba8());
        Handle::from_pixels(w, h, rgba.as_raw().clone())
    }
}

impl Bookmark {
    fn new(label: &str, path: &str) -> Self {
        Bookmark {
            label: label.into(),
            path: path.into(),
            id: CId::new(label.to_string()),
        }
    }
}

enum ClickType {
    Single,
    Double,
}
struct ClickTimer {
    idx: usize,
    time: Instant,
}
impl ClickTimer {
    fn click(self: &mut Self, idx: usize) -> ClickType {
        let time = Instant::now();
        if idx != self.idx || time - self.time > Duration::from_millis(300) {
            self.idx = idx;
            self.time = time;
            return ClickType::Single;
        }
        self.idx = idx;
        self.time = time;
        return ClickType::Double;
    }
}

pub struct SelectedTheme;
impl StyleSheet for SelectedTheme {
    type Style = iced::Theme;
    fn appearance(&self, _style: &Self::Style) -> Appearance {
        let mut appearance = Appearance {
            ..Appearance::default()
        };
        appearance.background = Some(Background::Color(color!(0x990000)));
        appearance
    }
}
pub fn get_sel_theme() -> Container {
    Container::Custom(
        Box::new(SelectedTheme) as Box<dyn StyleSheet<Style = iced::Theme>>
    )
}