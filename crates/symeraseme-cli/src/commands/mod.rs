//! Top-level command registration modules.

pub mod auto_confirm;
pub mod brokers;
pub mod calendar;
pub mod classify_reply;
pub mod completion;
pub mod config;
pub mod dashboard;
pub mod events;
pub mod generate_dashboard;
pub mod generate_rebuttal;
pub mod generate_report;
pub mod generate_scheduler;
pub mod grant;
pub mod help;
pub mod init_profile;
pub mod manual_tasks;
pub mod mcp;
pub mod migrate;
pub mod plan;
pub mod poll_inbox;
pub mod registry;
pub mod render_template;
pub mod requests;
pub mod review;
pub mod run_web_form;
pub mod schedule;
pub mod serve;
pub mod show_profile;
pub mod status;
pub mod tick;
pub mod version;

pub fn all_groups() -> &'static [&'static str] {
    &[
        auto_confirm::COMMAND_GROUP,
        brokers::COMMAND_GROUP,
        calendar::COMMAND_GROUP,
        classify_reply::COMMAND_GROUP,
        completion::COMMAND_GROUP,
        config::COMMAND_GROUP,
        dashboard::COMMAND_GROUP,
        events::COMMAND_GROUP,
        generate_dashboard::COMMAND_GROUP,
        generate_rebuttal::COMMAND_GROUP,
        generate_report::COMMAND_GROUP,
        generate_scheduler::COMMAND_GROUP,
        grant::COMMAND_GROUP,
        help::COMMAND_GROUP,
        init_profile::COMMAND_GROUP,
        manual_tasks::COMMAND_GROUP,
        mcp::COMMAND_GROUP,
        migrate::COMMAND_GROUP,
        plan::COMMAND_GROUP,
        poll_inbox::COMMAND_GROUP,
        registry::COMMAND_GROUP,
        render_template::COMMAND_GROUP,
        requests::COMMAND_GROUP,
        review::COMMAND_GROUP,
        run_web_form::COMMAND_GROUP,
        schedule::COMMAND_GROUP,
        serve::COMMAND_GROUP,
        show_profile::COMMAND_GROUP,
        status::COMMAND_GROUP,
        tick::COMMAND_GROUP,
        version::COMMAND_GROUP,
    ]
}
