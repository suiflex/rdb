//! In-app assistant, bound to a saved connection and database per conversation.

use std::{cell::RefCell, rc::Rc};

use rdb_agent::{
    AgentConfig, AgentOutcome, AgentScope, AgentStore, Conversation, Message, Mode, ModelRequest,
    Provider, ToolRunner,
};
use rdb_connstore::{Engine, EnvTag};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use crate::{AgentMessage, AppState, MainWindow};

#[derive(Default)]
struct UiRun {
    selected_id: Option<String>,
    active_config_id: Option<String>,
    pending_write: Option<PendingWrite>,
    abort: Option<tokio::task::AbortHandle>,
    generation: i32,
}

struct PendingWrite {
    conversation_id: String,
    connection_id: String,
    database: String,
    schema: Option<String>,
    sql: String,
}

fn paint(window: &MainWindow, store: &AgentStore, ui: &UiRun) {
    let titles = store
        .conversations()
        .iter()
        .map(|item| item.title.clone().into())
        .collect::<Vec<SharedString>>();
    let selected = ui
        .selected_id
        .as_ref()
        .and_then(|id| store.conversations().iter().position(|item| &item.id == id));
    window.set_agent_conversations(ModelRc::from(Rc::new(VecModel::from(titles))));
    window.set_agent_selected_conversation(selected.map(|i| i as i32).unwrap_or(-1));
    if let Some(conversation) = selected.map(|index| &store.conversations()[index]) {
        let messages = conversation
            .messages
            .iter()
            .filter(|message| message.role == "user" || message.role == "assistant")
            .map(|message| AgentMessage {
                role: message.role.clone().into(),
                text: message.content.clone().into(),
            })
            .collect::<Vec<_>>();
        window.set_agent_messages(ModelRc::from(Rc::new(VecModel::from(messages))));
        window.set_agent_mode(if conversation.mode == Mode::Agent {
            1
        } else {
            0
        });
        window.set_agent_database_name(conversation.database.clone().into());
    } else {
        window.set_agent_messages(ModelRc::from(Rc::new(VecModel::from(
            Vec::<AgentMessage>::new(),
        ))));
        window.set_agent_database_name("".into());
    }
    let pending = ui
        .pending_write
        .as_ref()
        .filter(|pending| ui.selected_id.as_deref() == Some(pending.conversation_id.as_str()));
    window.set_agent_proposed_write(
        pending
            .map(|pending| pending.sql.as_str())
            .unwrap_or_default()
            .into(),
    );
    window.set_agent_write_target(
        pending
            .map(|pending| {
                format!(
                    "{} / {}",
                    window.get_agent_connection_name(),
                    pending.database
                )
            })
            .unwrap_or_default()
            .into(),
    );
}

fn provider_from_label(label: &str) -> Option<Provider> {
    Some(match label {
        "OpenAI" => Provider::OpenAi,
        "Anthropic" => Provider::Anthropic,
        "Gemini" => Provider::Gemini,
        "DeepSeek" => Provider::DeepSeek,
        "Qwen" => Provider::Qwen,
        "MiniMax" => Provider::MiniMax,
        "OpenAI Compatible" => Provider::OpenAiCompatible,
        "Anthropic Compatible" => Provider::AnthropicCompatible,
        "Ollama" => Provider::Ollama,
        _ => return None,
    })
}

fn provider_label(provider: Provider) -> &'static str {
    match provider {
        Provider::OpenAi => "OpenAI",
        Provider::Anthropic => "Anthropic",
        Provider::Gemini => "Gemini",
        Provider::DeepSeek => "DeepSeek",
        Provider::Qwen => "Qwen",
        Provider::MiniMax => "MiniMax",
        Provider::OpenAiCompatible => "OpenAI Compatible",
        Provider::AnthropicCompatible => "Anthropic Compatible",
        Provider::Ollama => "Ollama",
    }
}

fn paint_config(window: &MainWindow, store: &AgentStore, ui: &UiRun) {
    let options = store
        .configs()
        .iter()
        .map(|item| item.name.clone().into())
        .collect::<Vec<SharedString>>();
    window.set_agent_config_options(ModelRc::from(Rc::new(VecModel::from(options))));
    if let Some(config) = store
        .configs()
        .iter()
        .find(|item| ui.active_config_id.as_deref() == Some(item.id.as_str()))
    {
        window.set_agent_config_choice(config.name.clone().into());
        window.set_agent_config_name(config.name.clone().into());
        window.set_agent_config_provider(provider_label(config.provider).into());
        window.set_agent_config_endpoint(config.endpoint.clone().into());
        window.set_agent_config_model(config.model.clone().into());
    } else {
        window.set_agent_config_choice("".into());
        window.set_agent_config_name("".into());
        window.set_agent_config_provider("OpenAI".into());
        window.set_agent_config_endpoint("https://api.openai.com/v1".into());
        window.set_agent_config_model("".into());
    }
    window.set_agent_config_key("".into());
}

pub(crate) fn wire(window: &MainWindow, state: &AppState) {
    let store = match AgentStore::open_default() {
        Ok(store) => Rc::new(RefCell::new(store)),
        Err(error) => {
            window.set_agent_status(format!("Agent storage: {error}").into());
            return;
        }
    };
    let ui = Rc::new(RefCell::new(UiRun::default()));
    ui.borrow_mut().active_config_id = store.borrow().configs().first().map(|item| item.id.clone());
    paint_config(window, &store.borrow(), &ui.borrow());

    {
        let store = store.clone();
        let ui = ui.clone();
        let conn_store = state.store.clone();
        let current = state.current_connection_id.clone();
        let weak = window.as_weak();
        window.on_agent_new_conversation(move || {
            let Some(w) = weak.upgrade() else { return };
            let Some(id) = current.lock().unwrap().clone() else {
                w.set_agent_status("Connect to a database first".into());
                return;
            };
            let Some(saved) = conn_store.borrow().get(&id).cloned() else {
                w.set_agent_status("Saved connection is unavailable".into());
                return;
            };
            let database = if saved.engine == Engine::Sqlite {
                "main".to_string()
            } else if saved.engine == Engine::Mongo && !w.get_bc_db().is_empty() {
                w.get_bc_db().to_string()
            } else {
                saved.database.clone().unwrap_or_default()
            };
            let schema = (!w.get_bc_schema().is_empty()).then(|| w.get_bc_schema().to_string());
            let conversation =
                Conversation::new(uuid::Uuid::new_v4().to_string(), id, database, schema);
            let new_id = conversation.id.clone();
            let saved_result = store.borrow_mut().save_conversation(conversation);
            match saved_result {
                Ok(()) => {
                    ui.borrow_mut().selected_id = Some(new_id);
                    w.set_agent_connection_name(saved.name.into());
                    w.set_agent_status("".into());
                    paint(&w, &store.borrow(), &ui.borrow());
                }
                Err(error) => w.set_agent_status(error.into()),
            }
        });
    }

    {
        let store = store.clone();
        let ui = ui.clone();
        let conn_store = state.store.clone();
        let weak = window.as_weak();
        window.on_agent_choose_conversation(move |index| {
            let Some(w) = weak.upgrade() else { return };
            let Some(conversation) = store
                .borrow()
                .conversations()
                .get(index.max(0) as usize)
                .cloned()
            else {
                return;
            };
            ui.borrow_mut().selected_id = Some(conversation.id);
            let name = conn_store
                .borrow()
                .get(&conversation.connection_id)
                .map(|saved| saved.name.clone())
                .unwrap_or_else(|| "Unavailable connection".into());
            w.set_agent_connection_name(name.into());
            paint(&w, &store.borrow(), &ui.borrow());
        });
    }

    {
        let store = store.clone();
        let ui = ui.clone();
        let conn_store = state.store.clone();
        let weak = window.as_weak();
        window.on_agent_delete_conversation(move |index| {
            let Some(w) = weak.upgrade() else { return };
            let Some(conversation) = store
                .borrow()
                .conversations()
                .get(index.max(0) as usize)
                .cloned()
            else {
                return;
            };
            if let Err(error) = store.borrow_mut().delete_conversation(&conversation.id) {
                w.set_agent_status(error.into());
                return;
            }
            let mut run = ui.borrow_mut();
            if run.selected_id.as_deref() == Some(conversation.id.as_str()) {
                run.generation = run.generation.wrapping_add(1);
                if let Some(abort) = run.abort.take() {
                    abort.abort();
                }
                run.selected_id = store
                    .borrow()
                    .conversations()
                    .last()
                    .map(|item| item.id.clone());
                w.set_agent_busy(false);
            }
            if run
                .pending_write
                .as_ref()
                .is_some_and(|pending| pending.conversation_id == conversation.id)
            {
                run.pending_write = None;
            }
            let name = run
                .selected_id
                .as_ref()
                .and_then(|id| {
                    store
                        .borrow()
                        .conversations()
                        .iter()
                        .find(|item| &item.id == id)
                        .map(|item| item.connection_id.clone())
                })
                .and_then(|id| conn_store.borrow().get(&id).map(|saved| saved.name.clone()))
                .unwrap_or_default();
            w.set_agent_connection_name(name.into());
            paint(&w, &store.borrow(), &run);
        });
    }

    {
        let store = store.clone();
        let ui = ui.clone();
        let weak = window.as_weak();
        window.on_agent_mode_changed(move |index| {
            let Some(w) = weak.upgrade() else { return };
            let Some(id) = ui.borrow().selected_id.clone() else {
                return;
            };
            let Some(mut conversation) = store
                .borrow()
                .conversations()
                .iter()
                .find(|item| item.id == id)
                .cloned()
            else {
                return;
            };
            conversation.mode = if index == 1 { Mode::Agent } else { Mode::Ask };
            if let Err(error) = store.borrow_mut().save_conversation(conversation) {
                w.set_agent_status(error.into());
            }
        });
    }

    {
        let store = store.clone();
        let ui = ui.clone();
        let weak = window.as_weak();
        window.on_agent_save_config(move |name, provider, endpoint, model, key| {
            let Some(w) = weak.upgrade() else { return };
            let Some(provider) = provider_from_label(&provider) else {
                w.set_agent_status("Unknown model provider".into());
                return;
            };
            if name.trim().is_empty() || endpoint.trim().is_empty() || model.trim().is_empty() {
                w.set_agent_status("Name, endpoint, and model are required".into());
                return;
            }
            let active_id = ui.borrow().active_config_id.clone();
            if store
                .borrow()
                .configs()
                .iter()
                .any(|item| item.name == name.as_str() && Some(&item.id) != active_id.as_ref())
            {
                w.set_agent_status("Model configuration name already exists".into());
                return;
            }
            let id = active_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
            let config = AgentConfig {
                id: id.clone(),
                name: name.to_string(),
                provider,
                endpoint: endpoint.to_string(),
                model: model.to_string(),
            };
            let result = store.borrow_mut().save_config(config, Some(&key));
            match result {
                Ok(()) => {
                    ui.borrow_mut().active_config_id = Some(id);
                    paint_config(&w, &store.borrow(), &ui.borrow());
                    w.set_agent_status("Model saved".into());
                }
                Err(error) => w.set_agent_status(error.into()),
            }
        });
    }

    {
        let store = store.clone();
        let ui = ui.clone();
        let weak = window.as_weak();
        window.on_agent_choose_config(move |name| {
            let Some(w) = weak.upgrade() else { return };
            ui.borrow_mut().active_config_id = store
                .borrow()
                .configs()
                .iter()
                .find(|item| item.name == name.as_str())
                .map(|item| item.id.clone());
            paint_config(&w, &store.borrow(), &ui.borrow());
        });
    }

    {
        let store = store.clone();
        let ui = ui.clone();
        let weak = window.as_weak();
        window.on_agent_new_config(move || {
            let Some(w) = weak.upgrade() else { return };
            ui.borrow_mut().active_config_id = None;
            paint_config(&w, &store.borrow(), &ui.borrow());
        });
    }

    {
        let store = store.clone();
        let ui = ui.clone();
        let weak = window.as_weak();
        window.on_agent_delete_config(move || {
            let Some(w) = weak.upgrade() else { return };
            let Some(id) = ui.borrow().active_config_id.clone() else {
                return;
            };
            if let Err(error) = store.borrow_mut().delete_config(&id) {
                w.set_agent_status(error.into());
                return;
            }
            ui.borrow_mut().active_config_id =
                store.borrow().configs().first().map(|item| item.id.clone());
            paint_config(&w, &store.borrow(), &ui.borrow());
            w.set_agent_status("Model configuration deleted".into());
        });
    }

    {
        let store = store.clone();
        let ui = ui.clone();
        let conn_store = state.store.clone();
        let focused_connection = state.current_connection_id.clone();
        let pool = state.driver_pool.clone();
        let current = state.current.clone();
        let rt = state.rt.clone();
        let weak = window.as_weak();
        window.on_agent_send(move |prompt| {
            let Some(w) = weak.upgrade() else { return };
            if w.get_agent_busy() || prompt.trim().is_empty() { return; }
            let needs_conversation = ui.borrow().selected_id.is_none();
            if needs_conversation { w.invoke_agent_new_conversation(); }
            let Some(id) = ui.borrow().selected_id.clone() else { return };
            let Some(mut conversation) = store.borrow().conversations().iter().find(|item| item.id == id).cloned() else { return };
            let Some(config) = store.borrow().configs().iter().find(|item|
                ui.borrow().active_config_id.as_deref() == Some(item.id.as_str())).cloned() else {
                w.set_agent_status("Configure a model in Agent settings first".into()); return;
            };
            let key = match store.borrow().api_key(&config.id) {
                Ok(key) => key.unwrap_or_default(),
                Err(error) => { w.set_agent_status(error.into()); return; }
            };
            let Some(saved) = conn_store.borrow().get(&conversation.connection_id).cloned() else {
                w.set_agent_status("Conversation connection was deleted".into()); return;
            };
            conversation.messages.push(Message { role: "user".into(), content: prompt.to_string() });
            if conversation.title == "New conversation" {
                conversation.title = prompt.chars().take(45).collect();
            }
            if let Err(error) = store.borrow_mut().save_conversation(conversation.clone()) {
                w.set_agent_status(error.into()); return;
            }
            paint(&w, &store.borrow(), &ui.borrow());
            w.set_agent_busy(true);
            w.set_agent_status("Contacting model…".into());
            let editor_context = if focused_connection.lock().unwrap().as_deref()
                == Some(conversation.connection_id.as_str())
                && (w.get_bc_db().is_empty()
                    || w.get_bc_db().as_str() == conversation.database.as_str())
            {
                w.get_query_text().to_string()
            } else {
                String::new()
            };
            let background_weak = weak.clone();
            let pool = pool.clone();
            let current = current.clone();
            let generation = {
                let mut run = ui.borrow_mut();
                run.generation = run.generation.wrapping_add(1);
                run.generation
            };
            let abort = rt.spawn(async move {
                let result = async {
                    let (_, driver) = crate::resolve_driver(&pool, &current, Some(&conversation.connection_id))
                        .await.ok_or("Connect the conversation's database first")?;
                    let scope = AgentScope { connection_id: conversation.connection_id.clone(),
                        database: conversation.database.clone(), schema: conversation.schema.clone(),
                        engine: saved.engine, production: saved.env_tag == EnvTag::Production };
                    let runner = ToolRunner::new(driver, scope);
                    let request = ModelRequest { config: &config, api_key: &key,
                        system: "You are RDB's database assistant. Follow the user's request and the configured mode.",
                        messages: &conversation.messages };
                    let (_, cancel) = tokio::sync::watch::channel(false);
                    rdb_agent::run(&request, conversation.mode, &runner, &editor_context, cancel).await
                }.await;
                let (answer, proposal, error) = match result {
                    Ok(run) => match run.outcome {
                        AgentOutcome::Answer(answer) => (answer, String::new(), String::new()),
                        AgentOutcome::WriteProposal(sql) => (String::new(), sql, String::new()),
                    },
                    Err(error) => (String::new(), String::new(), error),
                };
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = background_weak.upgrade() {
                        w.invoke_agent_result(id.into(), generation, answer.into(), proposal.into(), error.into());
                    }
                });
            });
            ui.borrow_mut().abort = Some(abort.abort_handle());
        });
    }

    {
        let store = store.clone();
        let ui = ui.clone();
        let weak = window.as_weak();
        window.on_agent_result(move |id, generation, answer, proposal, error| {
            let Some(w) = weak.upgrade() else { return };
            if generation != ui.borrow().generation {
                return;
            }
            ui.borrow_mut().abort = None;
            w.set_agent_busy(false);
            if !error.is_empty() {
                w.set_agent_status(error);
                return;
            }
            if !proposal.is_empty() {
                if let Some(conversation) = store
                    .borrow()
                    .conversations()
                    .iter()
                    .find(|item| item.id == id.as_str())
                    .cloned()
                {
                    ui.borrow_mut().pending_write = Some(PendingWrite {
                        conversation_id: conversation.id,
                        connection_id: conversation.connection_id,
                        database: conversation.database,
                        schema: conversation.schema,
                        sql: proposal.to_string(),
                    });
                    w.set_agent_status("Review the exact SQL and target before approval".into());
                    paint(&w, &store.borrow(), &ui.borrow());
                }
                return;
            }
            let selected = store
                .borrow()
                .conversations()
                .iter()
                .find(|item| item.id == id.as_str())
                .cloned();
            if let Some(mut conversation) = selected {
                conversation.messages.push(Message {
                    role: "assistant".into(),
                    content: answer.to_string(),
                });
                if let Err(error) = store.borrow_mut().save_conversation(conversation) {
                    w.set_agent_status(error.into());
                    return;
                }
            }
            w.set_agent_status("".into());
            paint(&w, &store.borrow(), &ui.borrow());
        });
    }

    {
        let ui = ui.clone();
        let weak = window.as_weak();
        window.on_agent_cancel(move || {
            let mut run = ui.borrow_mut();
            run.generation = run.generation.wrapping_add(1);
            if let Some(abort) = run.abort.take() {
                abort.abort();
            }
            if let Some(w) = weak.upgrade() {
                w.set_agent_busy(false);
                w.set_agent_status("Agent run stopped".into());
            }
        });
    }

    {
        let ui = ui.clone();
        let store = store.clone();
        let weak = window.as_weak();
        window.on_agent_reject_write(move || {
            ui.borrow_mut().pending_write = None;
            if let Some(w) = weak.upgrade() {
                paint(&w, &store.borrow(), &ui.borrow());
                w.set_agent_status("Write denied".into());
            }
        });
    }

    {
        let ui = ui.clone();
        let store = store.clone();
        let conn_store = state.store.clone();
        let pool = state.driver_pool.clone();
        let current = state.current.clone();
        let rt = state.rt.clone();
        let weak = window.as_weak();
        window.on_agent_approve_write(move || {
            let Some(w) = weak.upgrade() else { return };
            let Some(pending) = ui.borrow_mut().pending_write.take() else {
                return;
            };
            if ui.borrow().selected_id.as_deref() != Some(pending.conversation_id.as_str()) {
                ui.borrow_mut().pending_write = Some(pending);
                return;
            }
            let Some(saved) = conn_store.borrow().get(&pending.connection_id).cloned() else {
                w.set_agent_status("Connection was deleted".into());
                return;
            };
            if saved.env_tag == EnvTag::Production {
                w.set_agent_status("Agent writes are unavailable on production connections".into());
                return;
            }
            paint(&w, &store.borrow(), &ui.borrow());
            w.set_agent_busy(true);
            let background_weak = weak.clone();
            let pool = pool.clone();
            let current = current.clone();
            let generation = {
                let mut run = ui.borrow_mut();
                run.generation = run.generation.wrapping_add(1);
                run.generation
            };
            let abort = rt.spawn(async move {
                let result = async {
                    let (_, driver) =
                        crate::resolve_driver(&pool, &current, Some(&pending.connection_id))
                            .await
                            .ok_or("Connection is no longer open")?;
                    let scope = AgentScope {
                        connection_id: pending.connection_id.clone(),
                        database: pending.database.clone(),
                        schema: pending.schema.clone(),
                        engine: saved.engine,
                        production: saved.env_tag == EnvTag::Production,
                    };
                    ToolRunner::new(driver, scope)
                        .execute_approved_write(&pending.sql)
                        .await
                }
                .await;
                let (answer, error) = match result {
                    Ok(answer) => (answer, String::new()),
                    Err(error) => (String::new(), error),
                };
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = background_weak.upgrade() {
                        w.invoke_agent_write_result(
                            pending.conversation_id.into(),
                            generation,
                            answer.into(),
                            error.into(),
                        );
                    }
                });
            });
            ui.borrow_mut().abort = Some(abort.abort_handle());
        });
    }

    {
        let ui = ui.clone();
        let store = store.clone();
        let weak = window.as_weak();
        window.on_agent_write_result(move |conversation_id, generation, answer, error| {
            let Some(w) = weak.upgrade() else { return };
            if generation != ui.borrow().generation {
                return;
            }
            ui.borrow_mut().abort = None;
            w.set_agent_busy(false);
            if error.is_empty() {
                let selected = store
                    .borrow()
                    .conversations()
                    .iter()
                    .find(|item| item.id == conversation_id.as_str())
                    .cloned();
                if let Some(mut conversation) = selected {
                    conversation.messages.push(Message {
                        role: "assistant".into(),
                        content: format!("Write completed: {answer}"),
                    });
                    if let Err(error) = store.borrow_mut().save_conversation(conversation) {
                        w.set_agent_status(error.into());
                        return;
                    }
                }
                paint(&w, &store.borrow(), &ui.borrow());
            }
            w.set_agent_status(if error.is_empty() {
                format!("Write completed: {answer}").into()
            } else {
                error
            });
        });
    }

    paint(window, &store.borrow(), &ui.borrow());
}
