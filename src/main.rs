use std::time::Instant;

use wayrs_client::{global::GlobalsExt, protocol::*, Connection, EventCtx, IoMode};
use wayrs_protocols::xdg_shell::*;
use wayrs_utils::shm_alloc::{BufferSpec, ShmAlloc};

fn main() {
    let (mut conn, globals) = Connection::connect_and_collect_globals().unwrap();

    let wl_compositor: WlCompositor = globals.bind(&mut conn, ..=6).unwrap();
    let xdg_wm_base: XdgWmBase = globals.bind(&mut conn, ..=4).unwrap();
    let shm = ShmAlloc::bind(&mut conn, &globals).unwrap();

    let wl_surface = wl_compositor.create_surface(&mut conn);
    let xdg_surface = xdg_wm_base.get_xdg_surface_with_cb(&mut conn, wl_surface, xdg_surface_cb);
    let _xdg_toplevel = xdg_surface.get_toplevel_with_cb(&mut conn, xdg_toplevel_cb);
    wl_surface.commit(&mut conn);

    let mut state = State {
        exit: false,
        mapped: false,
        width: 500,
        height: 500,
        wl_surface,
        frame_cb: None,
        shm,
        time_anchor: Instant::now(),
    };

    while !state.exit {
        conn.flush(IoMode::Blocking).unwrap();
        conn.recv_events(IoMode::Blocking).unwrap();
        conn.dispatch_events(&mut state);
    }
}

struct State {
    exit: bool,
    mapped: bool,
    width: u32,
    height: u32,
    wl_surface: WlSurface,
    frame_cb: Option<WlCallback>,
    shm: ShmAlloc,
    time_anchor: Instant,
}

impl State {
    fn render(&mut self, conn: &mut Connection<Self>) {
        if !self.mapped || self.frame_cb.is_some() {
            return;
        }

        let (buf, canvas) = self.shm.alloc_buffer(
            conn,
            BufferSpec {
                width: self.width,
                height: self.height,
                stride: self.width * 4,
                format: wl_shm::Format::Argb8888,
            },
        );

        if self.time_anchor.elapsed().as_secs_f64() % 1.0 < 0.5 {
            self.wl_surface
                .set_buffer_transform(conn, wl_output::Transform::Normal);
        } else {
            self.wl_surface
                .set_buffer_transform(conn, wl_output::Transform::Flipped180);
        }

        let (top, bottom) = canvas.split_at_mut(self.width as usize * self.height as usize * 2);
        top.fill(255);
        bottom.fill(100);

        self.wl_surface
            .attach(conn, Some(buf.into_wl_buffer()), 0, 0);
        self.wl_surface.damage(conn, 0, 0, i32::MAX, i32::MAX);

        self.frame_cb = Some(self.wl_surface.frame_with_cb(conn, |ctx| {
            assert_eq!(ctx.state.frame_cb, Some(ctx.proxy));
            ctx.state.frame_cb = None;
            ctx.state.render(ctx.conn);
        }));

        self.wl_surface.commit(conn);
    }
}

fn xdg_surface_cb(ctx: EventCtx<State, XdgSurface>) {
    if let xdg_surface::Event::Configure(serial) = ctx.event {
        ctx.proxy.ack_configure(ctx.conn, serial);
        ctx.state.mapped = true;
        ctx.state.render(ctx.conn);
    };
}

fn xdg_toplevel_cb(ctx: EventCtx<State, XdgToplevel>) {
    use xdg_toplevel::Event as E;
    match ctx.event {
        E::Configure(args) => {
            if args.width != 0 {
                ctx.state.width = args.width.try_into().unwrap();
            }
            if args.height != 0 {
                ctx.state.height = args.height.try_into().unwrap();
            }
        }
        E::Close => {
            ctx.state.exit = true;
            ctx.conn.break_dispatch_loop();
        }
        _ => (),
    }
}
