//! Step executor
//!
//! Handles execution of individual steps, parallel blocks, loops, and conditionals.

use super::{PausedRun, StepContext};
use crate::adapter::{Adapter, AdapterRegistry};
use crate::dsl::{DependencyAnalyzer, Templater};
use crate::event::EventBus;
use crate::storage::Storage;
use crate::{BeemFlowError, Flow, Result, Step};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

/// Step executor
pub struct Executor {
    adapters: Arc<AdapterRegistry>,
    templater: Arc<Templater>,
    event_bus: Arc<dyn EventBus>,
    storage: Arc<dyn Storage>,
}

impl Executor {
    /// Create a new executor
    pub fn new(
        adapters: Arc<AdapterRegistry>,
        templater: Arc<Templater>,
        event_bus: Arc<dyn EventBus>,
        storage: Arc<dyn Storage>,
    ) -> Self {
        Self {
            adapters,
            templater,
            event_bus,
            storage,
        }
    }

    /// Execute steps starting from a given index
    ///
    /// Steps are executed in dependency order (topological sort), not YAML order.
    /// Dependencies are detected from:
    /// 1. Template references: `{{ steps.foo.output }}`
    /// 2. Manual `depends_on` fields
    pub async fn execute_steps(
        &self,
        flow: &Flow,
        step_ctx: &StepContext,
        start_idx: usize,
        run_id: Uuid,
    ) -> Result<HashMap<String, Value>> {
        // Pre-fetch previous run data for template access
        // This enables templates to use {{ runs.previous.outputs.step_name }}
        let runs_data = self.fetch_previous_run_data(&flow.name, run_id).await;
        if let Some(ref prev_data) = runs_data {
            // Store in step context for template rendering
            // Templates can access via {{ runs.previous.outputs.step1 }}
            step_ctx.set_var(
                "runs".to_string(),
                serde_json::to_value(serde_json::json!({"previous": prev_data}))
                    .unwrap_or(Value::Null),
            );
        }

        // Use dependency analyzer to determine execution order
        let analyzer = DependencyAnalyzer::new();
        let sorted_ids = analyzer.topological_sort(flow)?;

        // Create lookup map for steps
        let step_map: HashMap<String, &Step> = flow
            .steps
            .iter()
            .map(|s| (s.id.clone(), s))
            .collect();

        // Determine which step to start from
        // For fresh runs (start_idx=0), execute all steps in sorted order
        // For resumed runs, find the resume point in sorted order
        let sorted_start_idx = if start_idx == 0 {
            // Fresh run - start from beginning of sorted list
            0
        } else if start_idx < flow.steps.len() {
            // Resumed run - find the step to resume from in sorted order
            let start_step_id = &flow.steps[start_idx].id;
            sorted_ids
                .iter()
                .position(|id| id == start_step_id)
                .unwrap_or(0)
        } else {
            return Ok(step_ctx.snapshot().outputs);
        };

        // Execute steps in dependency order (starting from start_idx)
        for step_id in sorted_ids.iter().skip(sorted_start_idx) {
            let step = step_map
                .get(step_id)
                .ok_or_else(|| BeemFlowError::adapter(format!("step not found: {}", step_id)))?;

            // Handle await_event steps
            if step.await_event.is_some() {
                // Find original index for await_event handling
                let idx = flow.steps.iter().position(|s| &s.id == step_id).unwrap();
                return self
                    .handle_await_event(step, flow, step_ctx, idx, run_id)
                    .await;
            }

            // Execute regular step
            self.execute_single_step(step, step_ctx, &step.id).await?;

            // Persist step result
            self.persist_step_result(step, step_ctx, run_id).await?;
        }

        Ok(step_ctx.snapshot().outputs)
    }

    /// Fetch previous run data for template access
    async fn fetch_previous_run_data(
        &self,
        flow_name: &str,
        current_run_id: Uuid,
    ) -> Option<HashMap<String, Value>> {
        let runs_access = super::RunsAccess::new(
            self.storage.clone(),
            Some(current_run_id),
            flow_name.to_string(),
        );

        let prev_data = runs_access.previous().await;
        (!prev_data.is_empty()).then_some(prev_data)
    }

    /// Execute a step (public interface with boxing to avoid recursion issues)
    pub fn execute_step<'a>(
        &'a self,
        step: &'a Step,
        step_ctx: &'a StepContext,
        step_id: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + 'a>> {
        Box::pin(async move { self.execute_single_step(step, step_ctx, step_id).await })
    }

    /// Execute a single step (boxed to handle recursion)
    pub fn execute_single_step<'a>(
        &'a self,
        step: &'a Step,
        step_ctx: &'a StepContext,
        step_id: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + 'a>> {
        Box::pin(async move {
            // Check condition first
            if let Some(ref condition) = step.if_
                && !self.evaluate_condition(condition, step_ctx).await?
            {
                tracing::debug!(
                    "Skipping step {} - condition not met: {}",
                    step_id,
                    condition
                );
                return Ok(());
            }

            // Handle different step types
            if step.parallel == Some(true) && step.steps.is_some() {
                return self.execute_parallel_block(step, step_ctx, step_id).await;
            }

            if step.foreach.is_some() {
                return self.execute_foreach_block(step, step_ctx, step_id).await;
            }

            if step.wait.is_some() {
                return self.execute_wait(step).await;
            }

            if let Some(ref use_) = step.use_ {
                return self.execute_tool_call(use_, step, step_ctx, step_id).await;
            }

            Ok(())
        })
    }

    /// Execute a parallel block
    pub async fn execute_parallel_block(
        &self,
        step: &Step,
        step_ctx: &StepContext,
        step_id: &str,
    ) -> Result<()> {
        let steps = step
            .steps
            .as_ref()
            .ok_or_else(|| BeemFlowError::validation("parallel block must have steps"))?;

        let mut handles = Vec::new();

        for child_step in steps {
            let child = child_step.clone();
            let ctx = step_ctx.clone();
            let adapters = self.adapters.clone();
            let templater = self.templater.clone();

            let handle = tokio::spawn(async move {
                // Execute tool call directly for parallel steps (no nesting)
                if let Some(ref use_) = child.use_ {
                    let adapter = Self::resolve_adapter_static(&adapters, use_)?;
                    let mut inputs = Self::prepare_inputs_static(&templater, &child, &ctx)?;

                    // Add special __use parameter for core and MCP tools
                    if use_.starts_with(crate::constants::ADAPTER_PREFIX_CORE)
                        || use_.starts_with(crate::constants::ADAPTER_PREFIX_MCP)
                    {
                        inputs.insert(
                            crate::constants::PARAM_SPECIAL_USE.to_string(),
                            Value::String(use_.to_string()),
                        );
                    }

                    let outputs = adapter.execute(inputs).await?;
                    ctx.set_output(child.id.clone(), serde_json::to_value(outputs)?);
                }
                Ok::<_, BeemFlowError>((child.id.clone(), ctx.get_output(&child.id)))
            });

            handles.push(handle);
        }

        // Wait for all tasks to complete
        let mut outputs = HashMap::new();
        for handle in handles {
            let (child_id, output) = handle
                .await
                .map_err(|e| BeemFlowError::adapter(format!("parallel task failed: {}", e)))??;
            if let Some(output_val) = output {
                outputs.insert(child_id, output_val);
            }
        }

        step_ctx.set_output(step_id.to_string(), serde_json::to_value(outputs)?);
        Ok(())
    }

    /// Execute a foreach block
    pub async fn execute_foreach_block(
        &self,
        step: &Step,
        step_ctx: &StepContext,
        step_id: &str,
    ) -> Result<()> {
        // Extract required fields or return validation error
        let (foreach_expr, as_var, do_steps) = match (&step.foreach, &step.as_, &step.do_) {
            (Some(expr), Some(var), Some(steps)) => (expr, var, steps),
            (None, _, _) => return Err(BeemFlowError::validation("foreach expression missing")),
            (_, None, _) => return Err(BeemFlowError::validation("foreach must have 'as' field")),
            (_, _, None) => return Err(BeemFlowError::validation("foreach must have 'do' field")),
        };

        // Evaluate foreach expression
        let template_data = step_ctx.template_data();
        let list_val = self
            .templater
            .evaluate_expression(foreach_expr, &template_data)?;

        // Convert to array
        let list = list_val.as_array().ok_or_else(|| {
            BeemFlowError::validation(format!(
                "foreach expression did not evaluate to array: {:?}",
                list_val
            ))
        })?;

        if list.is_empty() {
            step_ctx.set_output(step_id.to_string(), Value::Object(serde_json::Map::new()));
            return Ok(());
        }

        // Execute in parallel or sequential
        if step.parallel == Some(true) {
            self.execute_foreach_parallel(list, as_var, do_steps, step_ctx)
                .await?;
        } else {
            self.execute_foreach_sequential(list, as_var, do_steps, step_ctx)
                .await?;
        }

        step_ctx.set_output(step_id.to_string(), Value::Object(serde_json::Map::new()));
        Ok(())
    }

    /// Execute foreach sequentially
    async fn execute_foreach_sequential(
        &self,
        list: &[Value],
        as_var: &str,
        do_steps: &[Step],
        step_ctx: &StepContext,
    ) -> Result<()> {
        for (index, item) in list.iter().enumerate() {
            // Set loop variables
            step_ctx.set_var(as_var.to_string(), item.clone());
            step_ctx.set_var(format!("{}_index", as_var), Value::Number(index.into()));
            step_ctx.set_var(format!("{}_row", as_var), Value::Number((index + 1).into()));

            // Execute all steps for this iteration
            for inner_step in do_steps {
                // Render step ID
                let template_data = step_ctx.template_data();
                let rendered_id = self
                    .render_value(&Value::String(inner_step.id.clone()), &template_data)?
                    .as_str()
                    .unwrap_or(&inner_step.id)
                    .to_string();

                self.execute_single_step(inner_step, step_ctx, &rendered_id)
                    .await?;
            }
        }

        Ok(())
    }

    /// Execute foreach in parallel
    async fn execute_foreach_parallel(
        &self,
        list: &[Value],
        as_var: &str,
        do_steps: &[Step],
        step_ctx: &StepContext,
    ) -> Result<()> {
        let mut handles = Vec::new();

        for (index, item) in list.iter().enumerate() {
            let item = item.clone();
            let as_var = as_var.to_string();
            let do_steps = do_steps.to_vec();
            let snapshot = step_ctx.snapshot();
            let adapters = self.adapters.clone();
            let templater = self.templater.clone();

            let handle = tokio::spawn(async move {
                // Create iteration context
                let iter_ctx = StepContext::new(snapshot.event, snapshot.vars, snapshot.secrets);

                // Copy existing outputs using iterator
                snapshot
                    .outputs
                    .into_iter()
                    .for_each(|(k, v)| iter_ctx.set_output(k, v));

                // Set loop variables
                iter_ctx.set_var(as_var.clone(), item);
                iter_ctx.set_var(format!("{}_index", as_var), Value::Number(index.into()));
                iter_ctx.set_var(format!("{}_row", as_var), Value::Number((index + 1).into()));

                // Execute steps - simple tool calls only in parallel foreach
                for inner_step in &do_steps {
                    if let Some(ref use_) = inner_step.use_ {
                        let adapter = Self::resolve_adapter_static(&adapters, use_)?;
                        let mut inputs =
                            Self::prepare_inputs_static(&templater, inner_step, &iter_ctx)?;

                        // Add special __use parameter for core and MCP tools
                        if use_.starts_with(crate::constants::ADAPTER_PREFIX_CORE)
                            || use_.starts_with(crate::constants::ADAPTER_PREFIX_MCP)
                        {
                            inputs.insert(
                                crate::constants::PARAM_SPECIAL_USE.to_string(),
                                Value::String(use_.to_string()),
                            );
                        }

                        let outputs = adapter.execute(inputs).await?;
                        iter_ctx.set_output(inner_step.id.clone(), serde_json::to_value(outputs)?);
                    }
                }

                Ok::<_, BeemFlowError>(iter_ctx.snapshot())
            });

            handles.push(handle);
        }

        // Wait for all iterations
        for handle in handles {
            let snapshot = handle.await.map_err(|e| {
                BeemFlowError::adapter(format!("foreach parallel task failed: {}", e))
            })??;

            // Merge outputs back to main context using iterator
            snapshot
                .outputs
                .into_iter()
                .for_each(|(k, v)| step_ctx.set_output(k, v));
        }

        Ok(())
    }

    /// Execute a tool call
    async fn execute_tool_call(
        &self,
        use_: &str,
        step: &Step,
        step_ctx: &StepContext,
        step_id: &str,
    ) -> Result<()> {
        // Resolve adapter
        let adapter = self.resolve_adapter(use_)?;

        // Prepare inputs
        let mut inputs = self.prepare_inputs(step, step_ctx).await?;

        // Add special __use parameter for core and MCP tools
        if use_.starts_with(crate::constants::ADAPTER_PREFIX_CORE)
            || use_.starts_with(crate::constants::ADAPTER_PREFIX_MCP)
        {
            inputs.insert(
                crate::constants::PARAM_SPECIAL_USE.to_string(),
                Value::String(use_.to_string()),
            );
        }

        // Execute with retry if configured
        let outputs = if let Some(ref retry) = step.retry {
            self.execute_with_retry(&adapter, inputs, retry).await?
        } else {
            adapter.execute(inputs).await?
        };

        // Store outputs
        step_ctx.set_output(step_id.to_string(), serde_json::to_value(outputs)?);

        Ok(())
    }

    /// Execute with retry logic and exponential backoff
    async fn execute_with_retry(
        &self,
        adapter: &Arc<dyn Adapter>,
        inputs: HashMap<String, Value>,
        retry: &crate::model::RetrySpec,
    ) -> Result<HashMap<String, Value>> {
        let mut attempts = 0;
        let mut last_error = None;

        while attempts < retry.attempts {
            match adapter.execute(inputs.clone()).await {
                Ok(outputs) => {
                    if attempts > 0 {
                        tracing::info!(
                            "Step succeeded on attempt {} after {} retries",
                            attempts + 1,
                            attempts
                        );
                    }
                    return Ok(outputs);
                }
                Err(e) => {
                    attempts += 1;
                    last_error = Some(e);

                    if attempts < retry.attempts {
                        let delay = self.calculate_retry_delay(attempts, retry.delay_sec);
                        tracing::debug!(
                            "Retrying step in {} seconds (attempt {} of {})",
                            delay,
                            attempts + 1,
                            retry.attempts
                        );
                        tokio::time::sleep(tokio::time::Duration::from_secs(delay)).await;
                    }
                }
            }
        }

        tracing::error!("Step failed after {} attempts", retry.attempts);
        Err(last_error.unwrap_or_else(|| BeemFlowError::adapter("retry failed")))
    }

    /// Calculate retry delay with exponential backoff
    fn calculate_retry_delay(&self, attempt: u32, base_delay: u64) -> u64 {
        // Exponential backoff: base_delay * 2^(attempt-1)
        // For attempt 1: base_delay * 1
        // For attempt 2: base_delay * 2
        // For attempt 3: base_delay * 4
        // etc.
        // Cap at 5 minutes maximum
        let delay = base_delay * (2_u64.pow(attempt - 1));
        delay.min(300) // Max 5 minutes
    }

    /// Execute a wait step
    pub async fn execute_wait(&self, step: &Step) -> Result<()> {
        if let Some(ref wait) = step.wait
            && let Some(seconds) = wait.seconds
        {
            tokio::time::sleep(tokio::time::Duration::from_secs(seconds)).await;
        }
        Ok(())
    }

    /// Handle await_event step
    async fn handle_await_event(
        &self,
        step: &Step,
        flow: &Flow,
        step_ctx: &StepContext,
        step_idx: usize,
        run_id: Uuid,
    ) -> Result<HashMap<String, Value>> {
        let await_spec = step
            .await_event
            .as_ref()
            .ok_or_else(|| BeemFlowError::validation("missing await_event specification"))?;

        // Extract and render token
        let token_val = await_spec
            .match_
            .get(crate::constants::MATCH_KEY_TOKEN)
            .ok_or_else(|| BeemFlowError::validation("await_event missing token in match"))?;

        let template_data = step_ctx.template_data();
        let rendered_token = self.render_value(token_val, &template_data)?;
        let token = rendered_token
            .as_str()
            .ok_or_else(|| BeemFlowError::validation("token must be a string"))?;

        // Validate that the token is not empty
        if token.trim().is_empty() {
            return Err(BeemFlowError::validation(
                "await_event token cannot be empty",
            ));
        }

        // Create paused run
        let paused = PausedRun {
            flow: flow.clone(),
            step_idx,
            context: step_ctx.clone(),
            outputs: step_ctx.snapshot().outputs,
            token: token.to_string(),
            run_id,
        };

        // Store paused run in storage (no in-memory cache)
        let paused_value = serde_json::to_value(&paused)?;
        self.storage.save_paused_run(token, paused_value).await?;

        // Set up event subscription with proper event matching
        let token_owned = token.to_string();
        let match_criteria = await_spec.match_.clone();
        let event_bus_ref = self.event_bus.clone();

        self.event_bus
            .subscribe(
                &await_spec.source,
                Arc::new(move |payload| {
                    // Check if this event matches our criteria
                    if Self::matches_event_criteria(&payload, &match_criteria) {
                        tracing::info!("Resume event matched for token: {}", token_owned);

                        // Trigger resume by publishing to the resume topic
                        let resume_topic = format!(
                            "{}{}",
                            crate::constants::EVENT_TOPIC_RESUME_PREFIX,
                            token_owned
                        );
                        let event_bus_clone = event_bus_ref.clone();
                        tokio::spawn(async move {
                            if let Err(e) = event_bus_clone.publish(&resume_topic, payload).await {
                                tracing::error!("Failed to publish resume event: {}", e);
                            }
                        });
                    }
                }),
            )
            .await?;

        // Note: The resume subscription is not set up here anymore
        // Instead, the Engine will call resume() manually via operations or the resume method
        // The flow now:
        // 1. Publish event to source topic (e.g., "test")
        // 2. This subscription matches and publishes to "resume.{token}"
        // 3. External code (tests, operations, etc.) must call engine.resume(token, event)
        //    OR subscribe to resume.* topics and call engine.resume()

        // For tests and manual resume, call engine.resume(token, event) directly

        // Handle timeout if specified
        if let Some(ref timeout) = await_spec.timeout {
            let timeout_duration = self.parse_timeout(timeout)?;
            let timeout_token = token.to_string();

            tokio::spawn(async move {
                tokio::time::sleep(timeout_duration).await;
                tracing::warn!("Timeout reached for await_event token: {}", timeout_token);

                // Timeout reached - could extend to trigger timeout event or mark run as failed
                // For now, just log the timeout as the subscription remains active
            });
        }

        Err(BeemFlowError::AwaitEventPause(format!(
            "step '{}' is waiting for event",
            step.id
        )))
    }

    /// Check if an event payload matches the specified criteria
    fn matches_event_criteria(
        payload: &serde_json::Value,
        criteria: &HashMap<String, serde_json::Value>,
    ) -> bool {
        criteria
            .iter()
            .filter(|(key, _)| *key != crate::constants::MATCH_KEY_TOKEN)
            .all(|(key, expected)| payload.get(key) == Some(expected))
    }

    /// Parse timeout string into Duration
    fn parse_timeout(&self, timeout: &str) -> Result<std::time::Duration> {
        // Simple timeout parsing - supports formats like "30s", "5m", "1h"
        let timeout_str = timeout.trim();

        let (value_str, multiplier) = if let Some(s) = timeout_str.strip_suffix('s') {
            (s, 1)
        } else if let Some(m) = timeout_str.strip_suffix('m') {
            (m, 60)
        } else if let Some(h) = timeout_str.strip_suffix('h') {
            (h, 3600)
        } else {
            return Err(BeemFlowError::validation(format!(
                "Unsupported timeout format: {}. Use '30s', '5m', or '1h'",
                timeout
            )));
        };

        value_str
            .parse::<u64>()
            .map(|v| std::time::Duration::from_secs(v * multiplier))
            .map_err(|_| BeemFlowError::validation(format!("Invalid timeout format: {}", timeout)))
    }

    /// Evaluate a conditional expression
    pub async fn evaluate_condition(
        &self,
        condition: &str,
        step_ctx: &StepContext,
    ) -> Result<bool> {
        // Condition must be in {{ }} format
        let trimmed = condition.trim();
        if !trimmed.starts_with("{{") || !trimmed.ends_with("}}") {
            return Err(BeemFlowError::validation(format!(
                "condition must use template syntax: {{{{ expression }}}}, got: {}",
                condition
            )));
        }

        // Use templater's evaluate_expression to get the actual value
        let template_data = step_ctx.template_data();
        let value = self
            .templater
            .evaluate_expression(condition, &template_data)?;

        // Check if it's a boolean
        if let Some(b) = value.as_bool() {
            return Ok(b);
        }

        // If it's a string that looks like a boolean
        if let Some(s) = value.as_str() {
            match s.to_lowercase().as_str() {
                "true" => return Ok(true),
                "false" => return Ok(false),
                _ => {}
            }
        }

        // For numbers, non-zero is truthy
        if let Some(n) = value.as_f64() {
            return Ok(n != 0.0);
        }

        // For arrays/objects, non-empty is truthy
        if value.is_array() {
            return Ok(!value.as_array().map(|a| a.is_empty()).unwrap_or(true));
        }
        if value.is_object() {
            return Ok(!value.as_object().map(|o| o.is_empty()).unwrap_or(true));
        }

        // Null is falsy
        Ok(!value.is_null())
    }

    /// Resolve adapter for a tool
    fn resolve_adapter(&self, tool_name: &str) -> Result<Arc<dyn Adapter>> {
        Self::resolve_adapter_static(&self.adapters, tool_name)
    }

    /// Static helper to resolve adapter
    fn resolve_adapter_static(
        adapters: &Arc<AdapterRegistry>,
        tool_name: &str,
    ) -> Result<Arc<dyn Adapter>> {
        // Try exact match first
        if let Some(adapter) = adapters.get(tool_name) {
            return Ok(adapter);
        }

        // Try by prefix
        if tool_name.starts_with(crate::constants::ADAPTER_PREFIX_MCP) {
            if let Some(adapter) = adapters.get(crate::constants::ADAPTER_ID_MCP) {
                return Ok(adapter);
            }
            return Err(BeemFlowError::adapter("MCP adapter not registered"));
        }

        if tool_name.starts_with(crate::constants::ADAPTER_PREFIX_CORE) {
            if let Some(adapter) = adapters.get(crate::constants::ADAPTER_ID_CORE) {
                return Ok(adapter);
            }
            return Err(BeemFlowError::adapter("Core adapter not registered"));
        }

        // Fallback to HTTP adapter for registry tools (e.g., http.fetch, openai.chat_completion)
        // This matches Go implementation where registry tools default to HTTP adapter
        if let Some(adapter) = adapters.get(crate::constants::HTTP_ADAPTER_ID) {
            return Ok(adapter);
        }

        Err(BeemFlowError::adapter(format!(
            "adapter not found: {} (and HTTP adapter not available)",
            tool_name
        )))
    }

    /// Prepare inputs for tool execution
    async fn prepare_inputs(
        &self,
        step: &Step,
        step_ctx: &StepContext,
    ) -> Result<HashMap<String, Value>> {
        Self::prepare_inputs_static(&self.templater, step, step_ctx)
    }

    /// Static helper to prepare inputs
    fn prepare_inputs_static(
        templater: &Arc<Templater>,
        step: &Step,
        step_ctx: &StepContext,
    ) -> Result<HashMap<String, Value>> {
        let template_data = step_ctx.template_data();

        step.with.as_ref().map_or_else(
            || Ok(HashMap::new()),
            |with| {
                with.iter()
                    .map(|(k, v)| {
                        Self::render_value_static(templater, v, &template_data)
                            .map(|rendered| (k.clone(), rendered))
                    })
                    .collect()
            },
        )
    }

    /// Render a value recursively
    fn render_value(&self, val: &Value, data: &HashMap<String, Value>) -> Result<Value> {
        Self::render_value_static(&self.templater, val, data)
    }

    /// Static helper to render value
    fn render_value_static(
        templater: &Arc<Templater>,
        val: &Value,
        data: &HashMap<String, Value>,
    ) -> Result<Value> {
        match val {
            Value::String(s) => templater.render(s, data).map(Value::String),
            Value::Array(arr) => arr
                .iter()
                .map(|elem| Self::render_value_static(templater, elem, data))
                .collect::<Result<Vec<_>>>()
                .map(Value::Array),
            Value::Object(obj) => obj
                .iter()
                .map(|(k, v)| {
                    Self::render_value_static(templater, v, data)
                        .map(|rendered| (k.clone(), rendered))
                })
                .collect::<Result<serde_json::Map<String, Value>>>()
                .map(Value::Object),
            _ => Ok(val.clone()),
        }
    }

    /// Persist step result to storage
    async fn persist_step_result(
        &self,
        step: &Step,
        step_ctx: &StepContext,
        run_id: Uuid,
    ) -> Result<()> {
        let outputs = step_ctx
            .get_output(&step.id)
            .and_then(|v| serde_json::from_value::<HashMap<String, Value>>(v).ok());

        let step_run = crate::model::StepRun {
            id: Uuid::new_v4(),
            run_id,
            step_name: step.id.clone(),
            status: crate::model::StepStatus::Succeeded,
            started_at: chrono::Utc::now(),
            ended_at: Some(chrono::Utc::now()),
            error: None,
            outputs,
        };

        self.storage.save_step(&step_run).await?;
        Ok(())
    }
}
