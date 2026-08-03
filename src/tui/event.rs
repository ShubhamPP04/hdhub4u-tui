use crate::tui::action::Action;
use crossterm::event::{Event as CrosstermEvent, KeyEventKind};
use std::time::Duration;
use tokio::sync::mpsc;

pub struct EventHandler {
    receiver: mpsc::Receiver<Action>,
}

impl EventHandler {
    pub fn new(tick_rate: Duration) -> Self {
        let (sender, receiver) = mpsc::channel(128);
        let event_sender = sender.clone();

        let mut tick_interval = tokio::time::interval(tick_rate);

        tokio::spawn(async move {
            let mut reader = crossterm::event::EventStream::new();
            use futures::StreamExt;

            loop {
                tokio::select! {
                    _ = tick_interval.tick() => {
                        let _ = event_sender.try_send(Action::Tick);
                    }
                    Some(event) = reader.next() => {
                        match event {
                            Ok(CrosstermEvent::Key(key)) => {
                                if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
                                    let _ = event_sender.send(Action::Key(key)).await;
                                }
                            }
                            Ok(CrosstermEvent::Mouse(mouse)) => {
                                match mouse.kind {
                                    crossterm::event::MouseEventKind::ScrollUp => {
                                        let _ = event_sender.try_send(Action::Key(crossterm::event::KeyEvent::new(crossterm::event::KeyCode::Up, crossterm::event::KeyModifiers::empty())));
                                    }
                                    crossterm::event::MouseEventKind::ScrollDown => {
                                        let _ = event_sender.try_send(Action::Key(crossterm::event::KeyEvent::new(crossterm::event::KeyCode::Down, crossterm::event::KeyModifiers::empty())));
                                    }
                                    _ => {}
                                }
                            }
                            Ok(CrosstermEvent::FocusGained) | Ok(CrosstermEvent::FocusLost) => {
                                let _ = event_sender.try_send(Action::FocusChange);
                            }
                            Ok(CrosstermEvent::Resize(w, h)) => {
                                let _ = event_sender.try_send(Action::Resize(w, h));
                            }
                            Err(error) => {
                                let _ = event_sender.send(Action::SetStatus(format!(
                                    "Error: terminal input failed: {error}"
                                ))).await;
                                break;
                            }
                            _ => {}
                        }
                    }
                }
            }
        });

        Self { receiver }
    }

    pub async fn next(&mut self) -> Option<Action> {
        self.receiver.recv().await
    }
}
