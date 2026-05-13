use std::ffi::CString;
use std::num::NonZeroU32;
use std::rc::Rc;
use std::time::Instant;

use bitflags::Flags;
use glutin::config::ConfigTemplateBuilder;
use glutin::display::GetGlDisplay;
use glutin::prelude::{GlDisplay, PossiblyCurrentGlContext};
use glutin::surface::GlSurface;
use glutin_winit::{DisplayBuilder, GlWindow};
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;
use winit::window::WindowId;

use crate::gl_handler::GlHandler;
use crate::glyph::{Glyph, GlyphAttribute};
use crate::terminal::{Term, TermMode};
use crate::win::{TermWindow, WinMode};

pub struct AppState {
    gl_surface: glutin::surface::Surface<glutin::surface::WindowSurface>,
    window: winit::window::Window,
}

pub struct App {
    gl_handler: GlHandler,
    app_state: Option<AppState>,
    gl: Option<Rc<glow::Context>>,

    term: Term,
    win: TermWindow,
    ttyfd: i32,
    rfd: libc::fd_set,
}

impl App {
    pub fn new(
        term: Term,
        win: TermWindow,
        template: ConfigTemplateBuilder,
        display_builder: DisplayBuilder,
    ) -> Self {
        let mut term = term;

        let ttyfd = term.ttynew(None, Some("/bin/zsh"), None, None);
        println!("ttyfd: {ttyfd}");

        Self {
            gl_handler: GlHandler::new(template, display_builder),
            app_state: None,
            gl: None,

            term,
            win,
            ttyfd,
            rfd: unsafe { std::mem::zeroed() },
        }
    }

    pub fn kpress(&mut self, event: KeyEvent) {
        if self.win.mode.contains(WinMode::MODE_KBDLOCK) {
            return;
        }

        // println!("key event: {:?}", event);

        let PhysicalKey::Code(code) = event.physical_key else {
            return;
        };

        // hightlight URLs when control held
        if code == KeyCode::ControlLeft {
            match event.state {
                ElementState::Pressed => {
                    println!("Control held");
                    // highlighturls();
                }
                ElementState::Released => {
                    println!("Control released");
                    // unhighlighturls();
                }
            }
        }

        // Released not relevant to shortcuts
        if event.state == ElementState::Released {
            return;
        }

        let is_alt_screen = self.term.tisaltscr();

        // shortcuts
        for shorcut in super::config::shortcuts {
            /*
             * TODO:
             * match shortcuts
             * if matches shortcut {
             * return;
             * }
             */
        }

        // custom keys from config
        /*
         * TODO:
         * if ((customkey = kmap(ksym, e->state))) {
         * 	ttywrite(customkey, strlen(customkey), 1);
         * 	return;
         * }
         */

        // composed string from input method
        let Some(text) = event.text_with_all_modifiers() else {
            return;
        };
        let mut len = text.len();

        if len == 0 {
            return;
        }
        println!("Composed text: {:?}", text);

        let mut buffer = ['\0'; 64];
        buffer[..len].copy_from_slice(&text.chars().take(64).collect::<Vec<_>>());

        if len == 1 {
            if self.win.mode.contains(WinMode::MODE_8BIT) {
                if (buffer[0] as u8) < 0177 {
                    let c = buffer[0] as u8 | 0x80;
                    len = (c as char).len_utf8();
                }
            } else {
                buffer[1] = '\0';
                buffer[0] = '\x1b';
                len = 2;
            }
        }

        self.term.ttywrite(&buffer, len, true);
        self.term.ttyread();
    }
    pub fn cmessage(&mut self) {}

    pub fn resize(&mut self, size: PhysicalSize<u32>) {
        if size.width == self.win.w && size.height == self.win.h {
            return;
        }

        self.gl_resize(size);
        // TODO: self.cresize(size.width, size.height);
    }

    fn gl_resize(&mut self, size: PhysicalSize<u32>) {
        if let Some(AppState {
            gl_surface,
            window: _window,
        }) = self.app_state.as_ref()
        {
            let gl_context = self.gl_handler.gl_context.as_ref().unwrap();
            gl_surface.resize(
                gl_context,
                NonZeroU32::new(size.width).unwrap(),
                NonZeroU32::new(size.height).unwrap(),
            );
        } else {
            println!("Resize event received before GL surface was created, ignoring.");
        }
    }

    fn cresize(&mut self, width: u32, height: u32) {
        if width != 0 {
            self.win.w = width;
        }

        if height != 0 {
            self.win.h = height;
        }

        // TODO:
        let borderpx = 0;
        let mut col = (self.win.w - 2 * borderpx) / self.win.cw;
        let mut row = (self.win.h - 2 * borderpx) / self.win.ch;

        col = col.max(2);
        row = row.min(1);

        self.win.hborderpx =
            ((self.win.w - col * self.win.cw) as f64 * super::config::HALIGN) as u32;

        self.win.vborderpx =
            ((self.win.h - row * self.win.ch) as f64 * super::config::VALIGN) as u32;

        self.term.tresize(col as usize, row as usize);

        // xresize(col, row);

        self.term
            .ttyresize(self.win.tw as usize, self.win.th as usize);
    }

    pub fn visibility(&mut self) {}
    pub fn unmap(&mut self) {}
    pub fn expose(&mut self) {}
    pub fn focus(&mut self) {}
    pub fn bmotion(&mut self) {}
    pub fn bpress(&mut self) {}
    pub fn brelease(&mut self) {}

    fn draw(&mut self) {
        let mut cx = self.term.c.x;
        let mut ocx = self.term.ocx;
        let mut ocy = self.term.c.y;

        if !self.xstartdraw() {
            return;
        }

        ocx = ocx.max(0).min(self.term.col - 1);
        ocy = ocy.max(0).min(self.term.row - 1);

        if self.term.line[self.term.ocy][self.term.ocx]
            .mode
            .contains(GlyphAttribute::ATTR_WDUMMY)
        {
            self.term.ocx -= 1;
        }

        if self.term.line[self.term.c.y][cx]
            .mode
            .contains(GlyphAttribute::ATTR_WDUMMY)
        {
            cx -= 1;
        }

        self.drawregion(0, 0, self.term.col, self.term.row);

        let line = self.term.line[self.term.ocy].clone();
        let g = &self.term.line[self.term.c.y][cx];
        let og = &raw mut self.term.line[self.term.ocy][self.term.ocx];

        self.xdrawcursor(
            cx as usize,
            self.term.c.y as usize,
            &self.term.line[self.term.c.y][cx],
            self.term.ocx as usize,
            self.term.ocy as usize,
            og,
            &line,
            self.term.col as usize,
        );

        self.term.ocx = cx;
        self.term.ocy = self.term.c.y;

        self.xfinishdraw();

        if ocx != self.term.ocx || ocy != self.term.ocy {
            self.xximspot(self.term.ocx as usize, self.term.ocy as usize);
        }
    }

    fn xstartdraw(&self) -> bool {
        return self.win.mode.contains(WinMode::MODE_VISIBLE);
    }

    fn drawregion(&mut self, x1: i32, y1: i32, x2: usize, y2: usize) {
        self.xstartimagedraw(&self.term.dirty, self.term.row);

        for y in y1 as usize..y2 {
            if !self.term.dirty[y] {
                continue;
            }

            self.term.dirty[y] = false;
            self.xdrawline(&self.term.line[y], x1, y, x2);
        }

        self.xfinishimagedraw();
    }

    fn xdrawcursor(
        &self,
        cx: usize,
        cy: usize,
        g: &Glyph,
        ox: usize,
        oy: usize,
        og: *mut Glyph,
        line: &[Glyph],
        len: usize,
    ) {
        // remove the old cursor
        if self.term.selected(ox, oy) {
            unsafe { (*og).mode.toggle(GlyphAttribute::ATTR_REVERSE) };
        }

        // Redraw the line where cursor was previously.
        // It will restore the ligatures broken by the cursor.

        self.xdrawline(line, 0, oy, len);
    }

    fn xfinishdraw(&self) {
        // TODO:
        println!("Finished drawing");
    }

    fn xximspot(&self, ocx: usize, ocy: usize) {
        // TODO:
        println!("xximspot");
    }

    fn xdrawline(&self, line: &[Glyph], arg: i32, oy: usize, len: usize) {
        let i = 0;
        let x = 0;
        let ox = 0;
        let numspecs = 0;

        let base: Glyph = Glyph::default();
        let new: Glyph = Glyph::default();
    }

    fn xstartimagedraw(&self, dirty: &[bool], row: usize) {
        // todo!()
    }

    fn xfinishimagedraw(&self) {
        // todo!()
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let gl_window = self.gl_handler.get_or_create_gl_window(event_loop);

        if gl_window.is_none() {
            return;
        }

        let (window, gl_config) = gl_window.unwrap();

        let attrs = window
            .build_surface_attributes(Default::default())
            .expect("Failed to build surface attributes");

        let gl_surface = unsafe {
            gl_config
                .display()
                .create_window_surface(&gl_config, &attrs)
                .expect("Failed to create surface")
        };

        let gl_context = self.gl_handler.gl_context.as_ref().unwrap();
        gl_context.make_current(&gl_surface).unwrap();

        let gl = unsafe {
            glow::Context::from_loader_function(|s| {
                let symbol = CString::new(s).unwrap();
                gl_config.display().get_proc_address(symbol.as_c_str())
            })
        };

        self.gl = Some(Rc::new(gl));

        self.win.mode.insert(WinMode::MODE_VISIBLE);
        //
        // self.triangle_renderer.get_or_insert_with(|| unsafe {
        //     renderers::TriangleRenderer::new(self.gl.as_ref().unwrap().clone())
        // });
        // self.text_renderer.get_or_insert_with(|| unsafe {
        //     TextRenderer::new(
        //         self.gl.as_ref().unwrap().clone(),
        //         &self.font_registry,
        //         FONT_SIZE,
        //     )
        // });

        // FontRegistry must outlive TextRenderer
        // self.text_renderer.get_or_insert_with(|| unsafe {
        //     // This is safe because the font registry is owned by the App struct
        //     // and will not be dropped while the TextRenderer is still in use.
        //     let font_registry: &'a FontRegistry = &*(&self.font_registry as *const _);
        //
        //     let width = window.inner_size().width as i32;
        //     let height = window.inner_size().height as i32;
        //     TextRenderer::<'a>::new(
        //         self.gl.as_ref().unwrap().clone(),
        //         font_registry,
        //         self.conf_font_size_px,
        //         (width, height),
        //     )
        // });
        //
        // self.quad_renderer.get_or_insert_with(|| unsafe {
        //     renderers::QuadRenderer::new(
        //         self.gl.as_ref().unwrap().clone(),
        //         window.inner_size().width as i32,
        //         window.inner_size().height as i32,
        //     )
        // });
        //
        self.app_state = Some(AppState { gl_surface, window });
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _id: WindowId,
        event: winit::event::WindowEvent,
    ) {
        let timeout: f64 = 0.0;

        match event {
            WindowEvent::KeyboardInput {
                event,
                is_synthetic: false,
                ..
            } => self.kpress(event),
            WindowEvent::Resized(size) => self.resize(size),

            WindowEvent::RedrawRequested => {
                let start = Instant::now();
                if let Some(AppState {
                    gl_surface,
                    window: _,
                }) = &self.app_state
                {
                    let gl_context = self.gl_handler.gl_context.as_ref().unwrap();

                    gl_surface.swap_buffers(gl_context).unwrap();
                }

                let duration = start.elapsed();
                println!("Redrawn in {} ms", duration.as_millis());
            }

            WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            _ => (),
        }

        unsafe {
            // TODO: implement missing timeout handling
            let tv: libc::timespec = libc::timespec {
                tv_sec: timeout as libc::time_t,
                tv_nsec: ((timeout - timeout.floor()) * 1e9) as libc::c_long,
            };

            libc::FD_ZERO(&mut self.rfd);
            libc::FD_SET(self.ttyfd, &mut self.rfd);

            if libc::pselect(
                self.ttyfd + 1,
                &mut self.rfd,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &tv,
                std::ptr::null(),
            ) < 0
            {
                if *libc::__errno_location() != libc::EINTR {
                    panic!("pselect failed: {}", std::io::Error::last_os_error());
                }
            }

            let ttyin = libc::FD_ISSET(self.ttyfd, &mut self.rfd); // || ttyread_pending();

            if ttyin {
                self.term.ttyread();
            }
        }

        self.draw();
    }
}
