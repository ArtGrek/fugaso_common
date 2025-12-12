use essential_core::{err_on, error::ServerError};
use log::LevelFilter;
use log4rs::append::console::ConsoleAppender;
use log4rs::append::rolling_file::policy::compound::roll::fixed_window::FixedWindowRoller;
use log4rs::append::rolling_file::policy::compound::trigger::size::SizeTrigger;
use log4rs::append::rolling_file::policy::compound::CompoundPolicy;
use log4rs::append::rolling_file::RollingFileAppender;
use log4rs::config::{Appender, Logger, Root};
use log4rs::encode::pattern::PatternEncoder;
use log4rs::Config;
use std::sync::Once;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn create_logger_config(name: &str) -> Config {
    let stdout = ConsoleAppender::builder()
        .encoder(Box::new(PatternEncoder::new("{d} {h({l})} {L} - {m}{n}")))
        .build();

    const MB_15: u64 = 15 * 1 << 20;
    let file_name = format!("logs/{name}/{VERSION}/application.log");

    let file = RollingFileAppender::builder()
        .build(
            &file_name,
            Box::new(CompoundPolicy::new(
                Box::new(SizeTrigger::new(MB_15)),
                Box::new(
                    FixedWindowRoller::builder()
                        .build(&format!("{file_name}.{{}}"), 400)
                        .unwrap(),
                ),
            )),
        )
        .unwrap();

    let config = Config::builder()
        .appender(Appender::builder().build("stdout", Box::new(stdout)))
        .appender(Appender::builder().build("file", Box::new(file)))
        .logger(Logger::builder().build("sqlx::query", LevelFilter::Warn))
        .logger(Logger::builder().build("fugaso_admin::dispatcher", LevelFilter::Info))
        .logger(Logger::builder().build("fugaso_math::math", LevelFilter::Warn))
        .logger(Logger::builder().build("fugaso_core::admin", LevelFilter::Warn))
        //.logger(Logger::builder().build("fugaso_core::proxy", LevelFilter::Info))
        .logger(Logger::builder().build("fugaso_math_ed1::math", LevelFilter::Warn))
        .logger(Logger::builder().build("fugaso_math_ed2::math", LevelFilter::Warn))
        .logger(Logger::builder().build("fugaso_math_ed3::math", LevelFilter::Warn))
        .logger(Logger::builder().build("fugaso_math_ed4::math", LevelFilter::Warn))
        .logger(Logger::builder().build("fugaso_math_ed5::math", LevelFilter::Warn))
        .build(
            Root::builder()
                .appender("stdout")
                .appender("file")
                .build(LevelFilter::Info),
        )
        .unwrap();
    config
}

static INIT: Once = Once::new();

pub fn setup_logger(name: &str) {
    INIT.call_once(|| {
        let logger_config = create_logger_config(name);
        log4rs::init_config(logger_config)
            .map_err(|e| err_on!(e))
            .expect("error logger init");
    })
}
