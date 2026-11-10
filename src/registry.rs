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

#[derive(Debug, Default, serde::Deserialize, JsonSchema)]
pub struct FeedbackFormsArgs {
    /// Filter feedback forms by class name, course code, or faculty name.
    pub class: Option<String>,
}

#[derive(Debug, serde::Deserialize, JsonSchema)]
pub struct QuizQuestionsArgs {
    /// Quiz title or numeric id.
    pub quiz: String,
    /// Disambiguate when multiple classes have quizzes.
    pub class: Option<String>,
}

#[derive(Debug, serde::Deserialize, JsonSchema)]
pub struct QuizAnswerArg {
    /// Numeric ID of the question.
    pub question_id: i64,
    /// Option IDs selected (for single or multi-choice questions).
    pub selected_options: Option<Vec<i64>>,
    /// Text response (for subjective questions).
    pub text_response: Option<String>,
}

#[derive(Debug, serde::Deserialize, JsonSchema)]
pub struct SubmitQuizArgs {
    /// Quiz title or numeric id.
    pub quiz: String,
    /// Class name, code or id if known.
    pub class: Option<String>,
    /// Array of question answers.
    pub answers: Vec<QuizAnswerArg>,
    /// If true, validates parameters and returns a preview payload without writing to the portal.
    pub dry_run: Option<bool>,
}

#[derive(Debug, serde::Deserialize, JsonSchema)]
pub struct SubmitAssignmentArgs {
    /// Assignment name or numeric id.
    pub assignment: String,
    /// Disambiguate when two classes have similarly named assignments.
    pub class: Option<String>,
    /// Text response, answer, or submission comments.
    pub text: Option<String>,
    /// Links or media keys of uploaded attachments.
    pub attachments: Option<Vec<String>>,
    /// If true, validates parameters and returns a preview payload without writing to the portal.
    pub dry_run: Option<bool>,
}

#[derive(Debug, serde::Deserialize, JsonSchema)]
pub struct FeedbackRatingArg {
    /// Question id in the feedback form.
    pub question_id: i64,
    /// Rating score (1 to 5).
    pub rating: u32,
    /// Optional remarks or feedback text.
    pub remarks: Option<String>,
}

#[derive(Debug, serde::Deserialize, JsonSchema)]
pub struct FillFeedbackArgs {
    /// Form numeric id or title.
    pub form: String,
    /// Ratings and remarks for each question.
    pub ratings: Vec<FeedbackRatingArg>,
    /// If true, validates parameters and returns a preview payload without writing to the portal.
    pub dry_run: Option<bool>,
}

#[derive(Debug, serde::Deserialize, JsonSchema)]
pub struct FillFormArgs {
    /// The form endpoint or path on the portal.
    pub form_path: String,
    /// Form field key-value pairs or structured payload.
    pub fields: serde_json::Value,
    /// If true, validates parameters and returns a preview payload without writing to the portal.
    pub dry_run: Option<bool>,
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

fn write_tool<A, F>(
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
    annotations.read_only_hint = Some(false);
    annotations.destructive_hint = Some(true);
    annotations.idempotent_hint = Some(false);
    annotations.open_world_hint = Some(false);
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

    router.add_route(tool::<FeedbackFormsArgs, _>(
        "feedback_forms",
        "Feedback questionnaires",
        "List pending course, faculty, and institutional feedback forms. Returns form titles, IDs, courses, faculty names, and submission deadlines.",
        |q, a: FeedbackFormsArgs| {
            Box::pin(finish(async move { q.feedback_forms(a.class.as_deref()).await }))
        },
    ));

    router.add_route(tool::<QuizQuestionsArgs, _>(
        "quiz_questions",
        "Quiz questions",
        "Retrieve questions and options for an active quiz so you can inspect them before answering.",
        |q, a: QuizQuestionsArgs| {
            Box::pin(finish(async move {
                q.quiz_questions(&a.quiz, a.class.as_deref()).await
            }))
        },
    ));

    router.add_route(write_tool::<SubmitAssignmentArgs, _>(
        "submit_assignment",
        "Submit assignment",
        "Submit an assignment with text and optional attachment links. Use `dry_run: true` to inspect the validated payload first for compliance.",
        |q, a: SubmitAssignmentArgs| {
            Box::pin(finish(async move {
                q.submit_assignment(
                    &a.assignment,
                    a.class.as_deref(),
                    a.text.as_deref(),
                    a.attachments.as_deref(),
                    a.dry_run,
                )
                .await
            }))
        },
    ));

    router.add_route(write_tool::<SubmitQuizArgs, _>(
        "submit_quiz",
        "Submit quiz",
        "Submit answers for an active quiz. Use `dry_run: true` to inspect the payload and verify compliance before final submission.",
        |q, a: SubmitQuizArgs| {
            let answers_json: Vec<serde_json::Value> = a
                .answers
                .iter()
                .map(|ans| {
                    serde_json::json!({
                        "questionId": ans.question_id,
                        "selectedOptions": ans.selected_options,
                        "textResponse": ans.text_response,
                    })
                })
                .collect();
            Box::pin(finish(async move {
                q.submit_quiz(&a.quiz, a.class.as_deref(), &answers_json, a.dry_run).await
            }))
        },
    ));

    router.add_route(write_tool::<FillFeedbackArgs, _>(
        "fill_feedback",
        "Fill feedback form",
        "Complete and submit a faculty or course feedback form. Use `dry_run: true` to verify ratings before submitting.",
        |q, a: FillFeedbackArgs| {
            let ratings_json: Vec<serde_json::Value> = a
                .ratings
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "questionId": r.question_id,
                        "rating": r.rating,
                        "remarks": r.remarks,
                    })
                })
                .collect();
            Box::pin(finish(async move {
                q.fill_feedback(&a.form, &ratings_json, a.dry_run).await
            }))
        },
    ));

    router.add_route(write_tool::<FillFormArgs, _>(
        "fill_form",
        "Fill portal form",
        "Generic agentic campus form filler. Use `dry_run: true` to inspect fields before submission.",
        |q, a: FillFormArgs| {
            Box::pin(finish(async move {
                q.fill_form(&a.form_path, &a.fields, a.dry_run).await
            }))
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
                "feedback_forms",
                "fill_feedback",
                "fill_form",
                "grades",
                "quiz_questions",
                "search",
                "submit_assignment",
                "submit_quiz",
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
        let write_tools = [
            "submit_assignment",
            "submit_quiz",
            "fill_feedback",
            "fill_form",
        ];
        for t in &tools {
            let is_write = write_tools.contains(&t.name.as_ref());
            assert_eq!(
                t.annotations.as_ref().and_then(|a| a.read_only_hint),
                Some(!is_write),
                "tool {} read_only_hint mismatch",
                t.name
            );
        }
    }
}
