// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! A scripted service on a thread of its own with its own GLib main loop, for
//! the fixtures the tests talk to over a socket or the bus. Dropping the
//! handle stops the loop and joins the thread.

use std::{sync::mpsc, thread, time::Duration};

pub struct ServiceThread {
    main_loop: glib::MainLoop,
    thread: Option<thread::JoinHandle<()>>,
}

impl ServiceThread {
    /// Starts the thread, runs `serve` on it inside its own context and then
    /// runs the main loop until the handle is dropped. `serve` returns what
    /// the test needs once the service listens, and a step that runs on the
    /// thread after the loop stopped; an error ends the thread instead.
    pub fn start<T: Send + 'static, E: Send + 'static>(
        serve: impl FnOnce(&glib::MainContext, &glib::MainLoop) -> Result<(T, Box<dyn FnOnce()>), E>
        + Send
        + 'static,
    ) -> Result<(Self, T), E> {
        let (ready, started) = mpsc::channel();
        let thread = thread::spawn(move || {
            let context = glib::MainContext::new();
            context
                .with_thread_default(|| {
                    let main_loop = glib::MainLoop::new(Some(&context), false);
                    match serve(&context, &main_loop) {
                        Ok((value, after_loop)) => {
                            ready.send(Ok((value, main_loop.clone()))).unwrap();
                            main_loop.run();
                            after_loop();
                        }
                        Err(error) => ready.send(Err(error)).unwrap(),
                    }
                })
                .unwrap();
        });
        match started
            .recv_timeout(Duration::from_secs(10))
            .expect("scripted service startup deadline")
        {
            Ok((value, main_loop)) => Ok((
                Self {
                    main_loop,
                    thread: Some(thread),
                },
                value,
            )),
            Err(error) => {
                thread.join().unwrap();
                Err(error)
            }
        }
    }
}

impl Drop for ServiceThread {
    fn drop(&mut self) {
        let main_loop = self.main_loop.clone();
        self.main_loop.context().invoke(move || main_loop.quit());
        self.thread.take().unwrap().join().unwrap();
    }
}
