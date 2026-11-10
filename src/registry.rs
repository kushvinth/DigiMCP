//! The explicit tool registry.
//!
//! Each entry names the tool, documents it for the model, and adapts the typed
//! arguments (whose JSON Schema is derived by `schemars`) onto the
//! implementation in [`crate::tools`].

use std::pin::Pin;
use std::sync::Arc;

use rmcp::handler::server::common::schema_for_input;
use rmcp::handler::server::router::tool::{ToolRoute, ToolRouter};
use rmcp::handler::server::tool::{CallToolHandler, ToolCallContext};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResponse, CallToolResult, ErrorData, Tool, ToolAnnotations};

use rmcp::ErrorData as McpError;
use schemars::JsonSchema;
use serde::de::DeserializeOwned;

use crate::server::{Quad, finish};
use crate::simplify::try_reduce;

#[derive(Debug, Default, serde::Deserialize, JsonSchema)]
pub struct NoArgs {}

#[derive(Debug, Default, serde::Deserialize, JsonSchema)]
pub struct ClassesArgs {
    /// Restrict to classes whose name or code contains this text.
    pub query: Option<String>,
}

#[derive(Debug, Default, serde::Deserialize, JsonSchema)]
pub struct DueArgs {
    /// Only work due within N hours (overdue items are always kept).
    pub within_hours: Option<f64>,
    /// Only this class (name, code or classId).
    pub class: Option<String>,
}

#[derive(Debug, Default, serde::Deserialize, JsonSchema)]
pub struct ListArgs {
    /// Only this class (name, code or classId).
    pub class: Option<String>,
}

#[derive(Debug, Default, serde::Deserialize, JsonSchema)]
pub struct DetailArgs {
    /// Assignment name or numeric id. Partial names are matched.
    pub assignment: String,
    /// Disambiguate when two classes have a similarly named assignment.
    pub class: Option<String>,
}

#[derive(Debug, Default, serde::Deserialize, JsonSchema)]
pub struct AttendanceArgs {
    /// Term name or id. Defaults to the current term.
    pub term: Option<String>,
    /// Only this class (name, code or classId).
    pub class: Option<String>,
}

#[derive(Debug, Default, serde::Deserialize, JsonSchema)]
pub struct GradesArgs {
    /// Only this class (name, code or classId). Omit for every class.
    pub class: Option<String>,
}

#[derive(Debug, Default, serde::Deserialize, JsonSchema)]
pub struct SearchArgs {
    /// Words to look for in assignment names and descriptions.
    pub query: String,
    /// Only this class (name, code or classId).
    pub class: Option<String>,
}

/// A typed tool handler: parse the arguments, run, serialise the answer.
type Runner<A> = dyn Fn(
        Quad,
        A,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<CallToolResult, ErrorData>> + Send>>
    + Send
    + Sync;

pub struct Handler<A> {
    run: Arc<Runner<A>>,
    _marker: std::marker::PhantomData<fn() -> A>,
}

impl<A> Clone for Handler<A> {
    fn clone(&self) -> Self {
        Self {
            run: Arc::clone(&self.run),
            _marker: std::marker::PhantomData,
        }
    }
}

impl<A> CallToolHandler<Quad, ()> for Handler<A>
where
    A: DeserializeOwned + Send + 'static,
{
    fn call(
        self,
        context: ToolCallContext<'_, Quad>,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<CallToolResponse, McpError>> + Send + '_>>
    {
        let this = context.service.clone();
        let arguments = context.arguments.clone();
        let run = Arc::clone(&self.run);
        Box::pin(async move {
            let args = arguments.unwrap_or_default();
            let parsed: A = serde_json::from_value(serde_json::Value::Object(args))
                .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
            let result = run(this, parsed).await?;
            let simplified = try_reduce(result);
            Ok(CallToolResponse::Complete(simplified))
        })
    }
}

fn tool<A, F>(
    name: &'static str,
    title: &'static str,
    description: &'static str,
    run: F,
) -> ToolRoute<Quad>
where
    A: DeserializeOwned + JsonSchema + Send + 'static,
    F: Fn(
            Quad,
            A,
        )
            -> Pin<Box<dyn std::future::Future<Output = Result<CallToolResult, ErrorData>> + Send>>
        + Send
        + Sync
        + Clone
        + 'static,
{
    let schema = schema_for_input::<Parameters<A>>().unwrap_or_else(|e| {
        panic!("invalid schema for tool {name}: {e}");
    });
    let mut annotations = ToolAnnotations::default();
    annotations.title = Some(title.to_string());
    annotations.read_only_hint = Some(true);
    annotations.destructive_hint = Some(false);
    annotations.idempotent_hint = Some(true);
    annotations.open_world_hint = Some(true);
    let attr = Tool::new(name, description, schema).annotate(annotations);
    ToolRoute::new(
        attr,
        Handler::<A> {
            run: Arc::new(run),
            _marker: std::marker::PhantomData,
        },
    )
}

/// Every tool, in the order agents see them.
pub fn router() -> ToolRouter<Quad> {
    let mut router = ToolRouter::new();

    router.add_route(tool::<NoArgs, _>(
        "account",
        "Who am I",
        "Connection and identity check: who you are signed in as, the portal and college, the timezone and server clock, and whether the session is still valid. Call this first if another tool reports a problem.",
        |q, _: NoArgs| Box::pin(finish(async move { q.account().await })),
    ));

    router.add_route(tool::<ClassesArgs, _>(
        "classes",
        "My classes",
        "List every class you are enrolled in. A course can appear once per lecture/practical/tutorial component, each with its own classId. Filter with `query` to resolve a subject name to an id.",
        |q, a: ClassesArgs| Box::pin(finish(async move { q.classes(a.query.as_deref()).await })),
    ));

    router.add_route(tool::<DueArgs, _>(
        "due",
        "What's due",
        "Everything still due, across every class and sorted by deadline: assignments, quizzes and exams in one list. Overdue-but-open and rejected work are included, with negative `hoursUntilDue` for anything overdue. Use `within_hours` for 'what is due this week' (168) or today (24).",
        |q, a: DueArgs| {
            Box::pin(finish(async move {
                q.due(a.within_hours, true, true, a.class.as_deref()).await
            }))
        },
    ));

    router.add_route(tool::<ListArgs, _>(
        "assignments",
        "Assignments and exams",
        "The full picture for every class or one: upcoming, previous and draft assignments with due dates and submission state, plus quizzes and exams with their windows. Use when the user wants a class-by-class view rather than a single deadline list.",
        |q, a: ListArgs| Box::pin(finish(async move { q.assignments(a.class.as_deref()).await })),
    ));

    router.add_route(tool::<DetailArgs, _>(
        "assignment",
        "Assignment detail",
        "Full detail for one assignment or assessment: description, due date, attachments with fresh signed links, submission and grading state, marks and remarks. Resolves the assignment by name, so ids are rarely needed.",
        |q, a: DetailArgs| {
            Box::pin(finish(async move {
                q.assignment(&a.assignment, a.class.as_deref()).await
            }))
        },
    ));

    router.add_route(tool::<AttendanceArgs, _>(
        "attendance",
        "Attendance",
        "Attendance for the current term (or a named one): overall percentage, per-course and per-component breakdown, plus `bunksLeft` — how many classes can still be missed while staying at or above the required minimum — and `bunksNeeded`, how many straight classes are needed to get back above it.",
        |q, a: AttendanceArgs| {
            Box::pin(finish(async move {
                q.attendance(a.term.as_deref(), a.class.as_deref()).await
            }))
        },
    ));

    router.add_route(tool::<GradesArgs, _>(
        "grades",
        "Grades",
        "Your results for every class (or one): published grade, assessment components and marks. Marks the portal has not published are reported as `published: false` rather than guessed.",
        |q, a: GradesArgs| Box::pin(finish(async move { q.grades(a.class.as_deref()).await })),
    ));

    router.add_route(tool::<SearchArgs, _>(
        "search",
        "Search assignments",
        "Search assignment titles and descriptions for a phrase across all classes (or one). Use when the user remembers what an assignment said, not what it was called.",
        |q, a: SearchArgs| {
            Box::pin(finish(async move { q.search(&a.query, a.class.as_deref()).await }))
        },
    ));

    router
}

#[cfg(test)]
mod tests {
    #[test]
    fn registers_every_tool() {
        let names: Vec<String> = super::router()
            .list_all()
            .iter()
            .map(|t| t.name.to_string())
            .collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(
            sorted,
            [
                "account",
                "assignment",
                "assignments",
                "attendance",
                "classes",
                "due",
                "grades",
                "search"
            ]
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
        );
    }

    #[test]
    fn schemas_advertise_their_arguments() {
        let tools = super::router().list_all();
        let due = tools.iter().find(|t| t.name == "due").unwrap();
        let props = due.input_schema["properties"].as_object().unwrap();
        assert!(props.contains_key("within_hours"), "{:?}", due.input_schema);
        assert!(props.contains_key("class"));
        let detail = tools.iter().find(|t| t.name == "assignment").unwrap();
        let required = detail.input_schema["required"].as_array().unwrap();
        assert!(required.iter().any(|r| r == "assignment"));
        for t in &tools {
            assert_eq!(
                t.annotations.as_ref().and_then(|a| a.read_only_hint),
                Some(true)
            );
        }
    }
}
