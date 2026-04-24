use agentix::{AgentEvent, LlmEvent, Message, Request, Tool, UserContent, agent, tool};
use futures::StreamExt;

struct Calculator;

#[tool]
impl agentix::Tool for Calculator {
    /// Add two numbers. Always return the exact numeric sum.
    /// a: first number
    /// b: second number
    async fn add(&self, a: f64, b: f64) -> f64 {
        a + b
    }

    /// Multiply two numbers. Always return the exact numeric product.
    /// a: first number
    /// b: second number
    async fn multiply(&self, a: f64, b: f64) -> f64 {
        a * b
    }
}

fn request() -> Request {
    Request::claude_code().model("sonnet").system_prompt(
        "You are a tool-call probe. You MUST call the provided add and multiply \
         tools for arithmetic. Do not calculate arithmetic mentally.",
    )
}

fn history() -> Vec<Message> {
    vec![Message::User(vec![UserContent::Text {
        text: "Use tools to compute (123 + 456) * 789, then answer with only the final number."
            .into(),
    }])]
}

fn tool_visibility_history() -> Vec<Message> {
    vec![Message::User(vec![UserContent::Text {
        text: "What tools can you see in this session? List their exact names if any are available. Do not call them.".into(),
    }])]
}

fn direct_call_history() -> Vec<Message> {
    vec![Message::User(vec![UserContent::Text {
        text: "Call the tool mcp__agentix__add with a=123 and b=456 now. Do not answer in text before calling the tool.".into(),
    }])]
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "agentix=debug,info".into()),
        )
        .init();

    let mode = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "agent".to_string());

    match mode.as_str() {
        "raw" => run_raw_stream().await?,
        "agent" => run_agent_loop().await?,
        "see" => run_tool_visibility_probe().await?,
        "call" => run_direct_call_probe().await?,
        "agent-call" => run_agent_direct_call_probe().await?,
        other => {
            eprintln!(
                "usage: cargo run --bin agentix_tool_probe -- [raw|agent|see|call|agent-call]"
            );
            anyhow::bail!("unknown mode: {other}");
        }
    }

    Ok(())
}

async fn run_agent_direct_call_probe() -> anyhow::Result<()> {
    eprintln!("agent direct-call probe");
    for tool in Calculator.raw_tools() {
        eprintln!(
            "tool: {} -> claude mcp name: mcp__agentix__{}",
            tool.function.name, tool.function.name
        );
    }

    let http = reqwest::Client::new();
    let mut stream = agent(
        Calculator,
        http,
        request(),
        direct_call_history(),
        Some(10_000),
    );

    let mut tool_starts = 0usize;
    let mut tool_results = 0usize;
    let mut final_text = String::new();

    while let Some(event) = stream.next().await {
        match event {
            AgentEvent::Token(t) => {
                print!("{t}");
                final_text.push_str(&t);
            }
            AgentEvent::Reasoning(t) => print!("[reasoning:{t}]"),
            AgentEvent::ToolCallChunk(c) => {
                println!(
                    "\nTOOL_CALL_CHUNK id={:?} name={:?} delta={:?}",
                    c.id, c.name, c.delta
                );
            }
            AgentEvent::ToolCallStart(tc) => {
                tool_starts += 1;
                println!(
                    "\nTOOL_CALL_START id={} name={} args={}",
                    tc.id, tc.name, tc.arguments
                );
            }
            AgentEvent::ToolProgress { id, name, progress } => {
                println!("\nTOOL_PROGRESS id={id} name={name} progress={progress}");
            }
            AgentEvent::ToolResult { id, name, content } => {
                tool_results += 1;
                let text = content
                    .iter()
                    .filter_map(|part| match part {
                        agentix::Content::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                println!("\nTOOL_RESULT id={id} name={name} content={text}");
            }
            AgentEvent::Usage(u) => {
                eprintln!(
                    "\nUSAGE prompt={} completion={} total={}",
                    u.prompt_tokens, u.completion_tokens, u.total_tokens
                );
            }
            AgentEvent::Warning(w) => eprintln!("\nWARNING {w}"),
            AgentEvent::Done(total) => {
                eprintln!("\nDONE total_tokens={}", total.total_tokens);
                break;
            }
            AgentEvent::Error(e) => {
                anyhow::bail!("agent error: {e}");
            }
        }
    }

    if tool_starts == 0 || tool_results == 0 {
        anyhow::bail!(
            "agent direct-call did not complete tool roundtrip: starts={tool_starts}, results={tool_results}"
        );
    }
    if !final_text.contains("579") {
        anyhow::bail!("final answer did not contain expected result 579: {final_text:?}");
    }

    Ok(())
}

async fn run_direct_call_probe() -> anyhow::Result<()> {
    let tools = Calculator.raw_tools();
    eprintln!("direct call probe: {} tools", tools.len());
    for tool in &tools {
        eprintln!(
            "registered tool: {} -> expected claude mcp name: mcp__agentix__{}",
            tool.function.name, tool.function.name
        );
    }

    let req = request().messages(direct_call_history()).tools(tools);
    let http = reqwest::Client::new();
    let mut stream = req.stream(&http).await?;
    let mut saw_tool_call = false;

    while let Some(event) = stream.next().await {
        match event {
            LlmEvent::Token(t) => print!("{t}"),
            LlmEvent::Reasoning(t) => print!("[reasoning:{t}]"),
            LlmEvent::ToolCallChunk(c) => {
                println!(
                    "\nTOOL_CALL_CHUNK id={:?} name={:?} delta={:?}",
                    c.id, c.name, c.delta
                );
            }
            LlmEvent::ToolCall(tc) => {
                saw_tool_call = true;
                println!(
                    "\nTOOL_CALL id={} name={} args={}",
                    tc.id, tc.name, tc.arguments
                );
            }
            LlmEvent::Usage(u) => {
                eprintln!(
                    "\nUSAGE prompt={} completion={} total={}",
                    u.prompt_tokens, u.completion_tokens, u.total_tokens
                );
            }
            LlmEvent::Done => {
                eprintln!("\nDONE");
                break;
            }
            LlmEvent::Error(e) => {
                eprintln!("\nERROR {e}");
                break;
            }
        }
    }

    if !saw_tool_call {
        anyhow::bail!("direct call stream ended without a complete ToolCall event");
    }

    Ok(())
}

async fn run_tool_visibility_probe() -> anyhow::Result<()> {
    let tools = Calculator.raw_tools();
    eprintln!("tool visibility probe: {} tools", tools.len());
    for tool in &tools {
        eprintln!(
            "registered tool: {} -> expected claude mcp name: mcp__agentix__{}",
            tool.function.name, tool.function.name
        );
    }

    let req = request().messages(tool_visibility_history()).tools(tools);
    let http = reqwest::Client::new();
    let mut stream = req.stream(&http).await?;

    while let Some(event) = stream.next().await {
        match event {
            LlmEvent::Token(t) => print!("{t}"),
            LlmEvent::Reasoning(t) => print!("[reasoning:{t}]"),
            LlmEvent::ToolCallChunk(c) => {
                println!(
                    "\nTOOL_CALL_CHUNK id={:?} name={:?} delta={:?}",
                    c.id, c.name, c.delta
                );
            }
            LlmEvent::ToolCall(tc) => {
                println!(
                    "\nTOOL_CALL id={} name={} args={}",
                    tc.id, tc.name, tc.arguments
                );
            }
            LlmEvent::Usage(u) => {
                eprintln!(
                    "\nUSAGE prompt={} completion={} total={}",
                    u.prompt_tokens, u.completion_tokens, u.total_tokens
                );
            }
            LlmEvent::Done => {
                eprintln!("\nDONE");
                break;
            }
            LlmEvent::Error(e) => {
                eprintln!("\nERROR {e}");
                break;
            }
        }
    }

    Ok(())
}

async fn run_raw_stream() -> anyhow::Result<()> {
    let tools = Calculator.raw_tools();
    eprintln!("raw stream probe: {} tools", tools.len());
    for tool in &tools {
        eprintln!(
            "tool: {} -> claude mcp name: mcp__agentix__{}",
            tool.function.name, tool.function.name
        );
    }

    let req = request().messages(history()).tools(tools);
    let http = reqwest::Client::new();
    let mut stream = req.stream(&http).await?;

    let mut saw_tool_call = false;
    while let Some(event) = stream.next().await {
        match event {
            LlmEvent::Token(t) => print!("{t}"),
            LlmEvent::Reasoning(t) => print!("[reasoning:{t}]"),
            LlmEvent::ToolCallChunk(c) => {
                println!(
                    "\nTOOL_CALL_CHUNK id={:?} name={:?} delta={:?}",
                    c.id, c.name, c.delta
                );
            }
            LlmEvent::ToolCall(tc) => {
                saw_tool_call = true;
                println!(
                    "\nTOOL_CALL id={} name={} args={}",
                    tc.id, tc.name, tc.arguments
                );
            }
            LlmEvent::Usage(u) => {
                eprintln!(
                    "\nUSAGE prompt={} completion={} total={}",
                    u.prompt_tokens, u.completion_tokens, u.total_tokens
                );
            }
            LlmEvent::Done => {
                eprintln!("\nDONE");
                break;
            }
            LlmEvent::Error(e) => {
                eprintln!("\nERROR {e}");
                break;
            }
        }
    }

    if !saw_tool_call {
        anyhow::bail!("raw stream ended without a complete ToolCall event");
    }
    Ok(())
}

async fn run_agent_loop() -> anyhow::Result<()> {
    eprintln!("agent loop probe");
    for tool in Calculator.raw_tools() {
        eprintln!(
            "tool: {} -> claude mcp name: mcp__agentix__{}",
            tool.function.name, tool.function.name
        );
    }

    let http = reqwest::Client::new();
    let mut stream = agent(Calculator, http, request(), history(), Some(10_000));

    let mut tool_starts = 0usize;
    let mut tool_results = 0usize;
    let mut final_text = String::new();

    while let Some(event) = stream.next().await {
        match event {
            AgentEvent::Token(t) => {
                print!("{t}");
                final_text.push_str(&t);
            }
            AgentEvent::Reasoning(t) => print!("[reasoning:{t}]"),
            AgentEvent::ToolCallChunk(c) => {
                println!(
                    "\nTOOL_CALL_CHUNK id={:?} name={:?} delta={:?}",
                    c.id, c.name, c.delta
                );
            }
            AgentEvent::ToolCallStart(tc) => {
                tool_starts += 1;
                println!(
                    "\nTOOL_CALL_START id={} name={} args={}",
                    tc.id, tc.name, tc.arguments
                );
            }
            AgentEvent::ToolProgress { id, name, progress } => {
                println!("\nTOOL_PROGRESS id={id} name={name} progress={progress}");
            }
            AgentEvent::ToolResult { id, name, content } => {
                tool_results += 1;
                let text = content
                    .iter()
                    .filter_map(|part| match part {
                        agentix::Content::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                println!("\nTOOL_RESULT id={id} name={name} content={text}");
            }
            AgentEvent::Usage(u) => {
                eprintln!(
                    "\nUSAGE prompt={} completion={} total={}",
                    u.prompt_tokens, u.completion_tokens, u.total_tokens
                );
            }
            AgentEvent::Warning(w) => eprintln!("\nWARNING {w}"),
            AgentEvent::Done(total) => {
                eprintln!("\nDONE total_tokens={}", total.total_tokens);
                break;
            }
            AgentEvent::Error(e) => {
                anyhow::bail!("agent error: {e}");
            }
        }
    }

    if tool_starts == 0 || tool_results == 0 {
        anyhow::bail!(
            "agent loop did not complete tool roundtrip: starts={tool_starts}, results={tool_results}"
        );
    }
    if !final_text.contains("456831") {
        anyhow::bail!("final answer did not contain expected result 456831: {final_text:?}");
    }

    Ok(())
}
