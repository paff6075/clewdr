use colored::Colorize;
use serde::Serialize;
use std::collections::{HashSet, VecDeque};
use tokio::{
    spawn,
    sync::{mpsc, oneshot},
    time::Interval,
};
use tracing::{error, info, warn};

use crate::{
    config::{CLEWDR_CONFIG, ClewdrConfig, CookieStatus, Reason, UselessCookie},
    error::ClewdrError,
};

const INTERVAL: u64 = 300;

#[derive(Debug, Serialize, Clone)]
pub struct CookieStatusInfo {
    pub valid: Vec<CookieStatus>,
    pub exhausted: Vec<CookieStatus>,
    pub invalid: Vec<UselessCookie>,
}

/// Unified event enum for cookie management with built-in priority ordering
#[derive(Debug)]
pub enum CookieEvent {
    /// Return a Cookie
    Return(CookieStatus, Option<Reason>),
    /// Submit a new Cookie
    Submit(CookieStatus),
    /// Check for timed out Cookies
    CheckReset,
    /// Request to get a Cookie
    Request(oneshot::Sender<Result<CookieStatus, ClewdrError>>),
    /// Get all Cookie status information
    GetStatus(oneshot::Sender<CookieStatusInfo>),
    /// Delete a Cookie
    Delete(CookieStatus, oneshot::Sender<Result<(), ClewdrError>>),
}
/// Cookie manager that handles cookie distribution, collection, and status tracking
pub struct CookieManager {
    valid: VecDeque<CookieStatus>,
    exhausted: HashSet<CookieStatus>,
    invalid: HashSet<UselessCookie>,
    event_rx: mpsc::Receiver<CookieEvent>, // Event receiver for incoming events
}

/// Event sender interface provided for external components to interact with the cookie manager
#[derive(Clone)]
pub struct CookieEventSender {
    sender: mpsc::Sender<CookieEvent>,
}

impl CookieEventSender {
    /// Request a cookie from the cookie manager
    ///
    /// # Returns
    /// * `Result<CookieStatus, ClewdrError>` - Cookie if available, error otherwise
    pub async fn request(&self) -> Result<CookieStatus, ClewdrError> {
        let (tx, rx) = oneshot::channel();
        self.sender.send(CookieEvent::Request(tx)).await?;
        rx.await?
    }

    /// Return a cookie to the cookie manager with optional reason
    ///
    /// # Arguments
    /// * `cookie` - The cookie to return
    /// * `reason` - Optional reason for returning the cookie (e.g., invalid, restricted)
    ///
    /// # Returns
    /// Result indicating success or send error
    pub async fn return_cookie(
        &self,
        cookie: CookieStatus,
        reason: Option<Reason>,
    ) -> Result<(), mpsc::error::SendError<CookieEvent>> {
        self.sender.send(CookieEvent::Return(cookie, reason)).await
    }

    /// Submit a new cookie to the cookie manager
    ///
    /// # Arguments
    /// * `cookie` - The new cookie to add
    ///
    /// # Returns
    /// Result indicating success or send error
    pub async fn submit(
        &self,
        cookie: CookieStatus,
    ) -> Result<(), mpsc::error::SendError<CookieEvent>> {
        self.sender.send(CookieEvent::Submit(cookie)).await
    }

    /// Get status information about all cookies
    ///
    /// # Returns
    /// * `Result<CookieStatusInfo, ClewdrError>` - Status information about all cookies
    pub async fn get_status(&self) -> Result<CookieStatusInfo, ClewdrError> {
        let (tx, rx) = oneshot::channel();
        self.sender.send(CookieEvent::GetStatus(tx)).await?;
        Ok(rx.await?)
    }

    /// Delete a cookie from the cookie manager
    ///
    /// # Arguments
    /// * `cookie` - The cookie to delete
    ///
    /// # Returns
    /// * `Result<(), ClewdrError>` - Success or error
    pub async fn delete_cookie(&self, cookie: CookieStatus) -> Result<(), ClewdrError> {
        let (tx, rx) = oneshot::channel();
        self.sender.send(CookieEvent::Delete(cookie, tx)).await?;
        rx.await?
    }

    /// Used for internal reset checking
    /// Sends a reset check event to the cookie manager
    ///
    /// # Returns
    /// Result indicating success or send error
    pub(crate) async fn check_reset(&self) -> Result<(), mpsc::error::SendError<CookieEvent>> {
        self.sender.send(CookieEvent::CheckReset).await
    }
}

impl CookieManager {
    /// Starts the cookie manager and returns an event sender
    ///
    /// Initializes cookie collections, creates event channels and queues,
    /// and spawns the event processing task
    ///
    /// # Returns
    /// * `CookieEventSender` - Event sender for interacting with the cookie manager
    pub fn start() -> CookieEventSender {
        let valid = VecDeque::from_iter(
            CLEWDR_CONFIG
                .load()
                .cookie_array
                .iter()
                .filter(|c| c.reset_time.is_none())
                .cloned(),
        );
        let exhaust = HashSet::from_iter(
            CLEWDR_CONFIG
                .load()
                .cookie_array
                .iter()
                .filter(|c| c.reset_time.is_some())
                .cloned(),
        );
        let invalid = HashSet::from_iter(CLEWDR_CONFIG.load().wasted_cookie.iter().cloned());

        // 创建事件通道
        let (event_tx, event_rx) = mpsc::channel(100);

        let sender = CookieEventSender { sender: event_tx };

        let manager = Self {
            valid,
            exhausted: exhaust,
            invalid,
            event_rx,
        };
        // 启动事件处理器
        spawn(manager.run(sender.to_owned()));

        sender
    }

    /// Logs the current state of cookie collections
    /// Displays counts of valid, exhausted, and invalid cookies
    fn log(&self) {
        info!(
            "Valid: {}, Exhausted: {}, Invalid: {}",
            self.valid.len().to_string().green(),
            self.exhausted.len().to_string().yellow(),
            self.invalid.len().to_string().red(),
        );
    }

    /// Saves the current state of cookies to the configuration
    /// Updates the cookie arrays in the config and writes to disk
    fn save(&mut self) {
        CLEWDR_CONFIG.rcu(|config| {
            let mut config = ClewdrConfig::clone(config);
            config.cookie_array = self
                .valid
                .iter()
                .chain(self.exhausted.iter())
                .cloned()
                .collect();
            config.wasted_cookie = self.invalid.iter().cloned().collect();
            config
        });
        CLEWDR_CONFIG.load().save().unwrap_or_else(|e| {
            error!("Failed to save config: {}", e);
        });
    }

    /// Checks and resets cookies that have passed their reset time
    /// Moves reset cookies from exhausted to valid collection
    fn reset(&mut self) {
        let mut reset_cookies = Vec::new();
        self.exhausted.retain(|cookie| {
            let reset_cookie = cookie.to_owned().reset();
            if reset_cookie.reset_time.is_none() {
                reset_cookies.push(reset_cookie);
                false
            } else {
                true
            }
        });
        if reset_cookies.is_empty() {
            return;
        }
        self.valid.extend(reset_cookies);
        self.log();
        self.save();
    }

    /// Dispatches a cookie for use
    /// Gets a cookie from the valid collection
    ///
    /// # Returns
    /// * `Result<CookieStatus, ClewdrError>` - A cookie if available, error otherwise
    fn dispatch(&mut self) -> Result<CookieStatus, ClewdrError> {
        self.reset();
        let cookie = self
            .valid
            .pop_front()
            .ok_or(ClewdrError::NoCookieAvailable)?;
        self.valid.push_back(cookie.to_owned());
        Ok(cookie)
    }

    /// Collects a returned cookie and processes it based on the return reason
    ///
    /// # Arguments
    /// * `cookie` - The cookie being returned
    /// * `reason` - Optional reason for the return that determines how the cookie is processed
    fn collect(&mut self, mut cookie: CookieStatus, reason: Option<Reason>) {
        let Some(reason) = reason else {
            return;
        };
        let mut find_remove = |cookie: &CookieStatus| {
            self.valid.retain(|c| c != cookie);
        };
        match reason {
            Reason::NormalPro => {
                return;
            }
            Reason::TooManyRequest(i) => {
                find_remove(&cookie);
                cookie.reset_time = Some(i);
                if !self.exhausted.insert(cookie) {
                    return;
                }
            }
            Reason::Restricted(i) => {
                find_remove(&cookie);
                cookie.reset_time = Some(i);
                if !self.exhausted.insert(cookie) {
                    return;
                }
            }
            Reason::NonPro => {
                find_remove(&cookie);
                if !self
                    .invalid
                    .insert(UselessCookie::new(cookie.cookie, reason))
                {
                    return;
                }
            }
            _ => {
                find_remove(&cookie);
                if !self
                    .invalid
                    .insert(UselessCookie::new(cookie.cookie, reason))
                {
                    return;
                }
            }
        }
        self.save();
        self.log();
    }

    /// Accepts a new cookie into the valid collection
    /// Checks for duplicates before adding
    ///
    /// # Arguments
    /// * `cookie` - The new cookie to accept
    fn accept(&mut self, cookie: CookieStatus) {
        if CLEWDR_CONFIG.load().cookie_array.contains(&cookie)
            || CLEWDR_CONFIG
                .load()
                .wasted_cookie
                .iter()
                .any(|c| *c == cookie)
        {
            warn!("Cookie already exists");
            return;
        }
        self.valid.push_back(cookie.to_owned());
        self.save();
        self.log();
    }

    /// Creates a report of all cookie statuses
    ///
    /// # Returns
    /// * `CookieStatusInfo` - Information about all cookie collections
    fn report(&self) -> CookieStatusInfo {
        CookieStatusInfo {
            valid: self.valid.to_owned().into(),
            exhausted: self.exhausted.iter().cloned().collect(),
            invalid: self.invalid.iter().cloned().collect(),
        }
    }

    /// Deletes a cookie from all collections
    ///
    /// # Arguments
    /// * `cookie` - The cookie to delete
    ///
    /// # Returns
    /// * `Result<(), ClewdrError>` - Success if found and deleted, error otherwise
    fn delete(&mut self, cookie: CookieStatus) -> Result<(), ClewdrError> {
        let mut found = false;
        self.valid.retain(|c| {
            found |= *c == cookie;
            *c != cookie
        });
        let useless = UselessCookie::new(cookie.cookie.to_owned(), Reason::Null);
        found = self.exhausted.remove(&cookie) | self.invalid.remove(&useless);

        if found {
            // Update config to reflect changes
            self.save();
            self.log();
            Ok(())
        } else {
            Err(ClewdrError::UnexpectedNone)
        }
    }

    /// Spawns a task to listen for timer events and send timeout check events
    ///
    /// # Arguments
    /// * `interval` - The time interval for periodic checks
    /// * `event_tx` - Event sender to send timeout check events
    fn spawn_timeout_checker(mut interval: Interval, event_tx: CookieEventSender) {
        tokio::spawn(async move {
            loop {
                interval.tick().await;
                if event_tx.check_reset().await.is_err() {
                    break;
                }
            }
        });
    }

    /// Main event processing loop
    /// Starts event receivers and processes events based on priority
    ///
    /// # Arguments
    /// * `event_rx` - Event receiver for incoming events
    /// * `event_sender` - Event sender for timeout checking
    async fn run(mut self, event_sender: CookieEventSender) {
        // 启动超时检查协程
        let interval = tokio::time::interval(tokio::time::Duration::from_secs(INTERVAL));
        Self::spawn_timeout_checker(interval, event_sender);

        // 事件处理主循环
        self.log();
        while let Some(res) = self.event_rx.recv().await {
            // 尝试从队列中获取事件
            match res {
                // 处理事件
                CookieEvent::Return(cookie, reason) => {
                    // 处理返回的cookie (最高优先级)
                    self.collect(cookie, reason);
                }
                CookieEvent::Submit(cookie) => {
                    // 处理提交的新cookie (次高优先级)
                    self.accept(cookie);
                }
                CookieEvent::CheckReset => {
                    // 处理超时检查 (中等优先级)
                    self.reset();
                }
                CookieEvent::Request(sender) => {
                    // 处理请求 (最低优先级)
                    let cookie = self.dispatch();
                    sender.send(cookie).unwrap_or_else(|_| {
                        error!("Failed to send cookie");
                    });
                }
                CookieEvent::GetStatus(sender) => {
                    let status_info = self.report();
                    sender.send(status_info).unwrap_or_else(|_| {
                        error!("Failed to send status info");
                    });
                }
                CookieEvent::Delete(cookie, sender) => {
                    let result = self.delete(cookie);
                    sender.send(result).unwrap_or_else(|_| {
                        error!("Failed to send delete result");
                    });
                }
            }
        }
    }
}
