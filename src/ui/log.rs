use lazy_static::lazy_static;
use std::{
    collections::HashMap,
    fmt,
    sync::{Arc, RwLock},
};

use tracing_subscriber::layer::Layer;

lazy_static! {
    static ref UI_LOGGER: Arc<RwLock<UiLogger>> = Arc::new(RwLock::new(UiLogger::default()));
}

#[derive(Default)]
struct UiLogger {
    logs: Vec<(String, tracing::Level)>,
}

impl UiLogger {
    pub fn log(&mut self, record: Record) {
        let s = format!("{}", record.args);
        println!("{}: {}", record.level, s);
        self.logs.push((s, record.level));
    }
}

#[derive(Clone, Debug)]
pub struct Record<'a> {
    level: tracing::Level,
    args: fmt::Arguments<'a>,
}

pub struct UITracingSubscriberLayer;

impl<S> Layer<S> for UITracingSubscriberLayer
where
    S: tracing::Subscriber,
{
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let mut visitor = ToStringVisitor::default();
        event.record(&mut visitor);

        let level = *event.metadata().level();

        UI_LOGGER.write().unwrap().log(Record {
            level,
            args: format_args!("{}", visitor),
        });
    }
}

#[derive(Default)]
struct ToStringVisitor<'a>(HashMap<&'a str, String>);

impl fmt::Display for ToStringVisitor<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0
            .iter()
            .try_for_each(|(k, v)| -> fmt::Result { write!(f, " {}: {}", k, v) })
    }
}

impl tracing::field::Visit for ToStringVisitor<'_> {
    fn record_f64(&mut self, field: &tracing::field::Field, value: f64) {
        self.0
            .insert(field.name(), format_args!("{}", value).to_string());
    }

    fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
        self.0
            .insert(field.name(), format_args!("{}", value).to_string());
    }

    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        self.0
            .insert(field.name(), format_args!("{}", value).to_string());
    }

    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        self.0
            .insert(field.name(), format_args!("{}", value).to_string());
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.0
            .insert(field.name(), format_args!("{}", value).to_string());
    }

    fn record_error(
        &mut self,
        field: &tracing::field::Field,
        value: &(dyn std::error::Error + 'static),
    ) {
        self.0
            .insert(field.name(), format_args!("{}", value).to_string());
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.0
            .insert(field.name(), format_args!("{:?}", value).to_string());
    }
}
