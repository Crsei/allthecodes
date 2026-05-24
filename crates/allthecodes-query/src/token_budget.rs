use allthecodes_engine::types::state::{BudgetCompletionEvent, BudgetTracker, TokenBudgetDecision};

const COMPLETION_THRESHOLD: f64 = 0.9;
const DIMINISHING_THRESHOLD: u64 = 500;

/// 妫€鏌?token 棰勭畻, 鍐冲畾鏄惁缁х画
///
/// 瀵瑰簲 TypeScript: query/tokenBudget.ts 鐨?checkTokenBudget
pub fn check_token_budget(
    tracker: &mut BudgetTracker,
    agent_id: Option<&str>,
    budget: Option<u64>,
    global_turn_tokens: u64,
) -> TokenBudgetDecision {
    // 瀛愪唬鐞嗘垨鏃犻绠?鈫?鍋滄
    let budget = match budget {
        Some(b) if b > 0 && agent_id.is_none() => b,
        _ => {
            return TokenBudgetDecision::Stop {
                completion_event: None,
            };
        }
    };

    let turn_tokens = global_turn_tokens;
    let pct = ((turn_tokens as f64 / budget as f64) * 100.0).round() as usize;
    let delta_since_last = global_turn_tokens.saturating_sub(tracker.last_global_turn_tokens);

    let is_diminishing = tracker.continuation_count >= 3
        && delta_since_last < DIMINISHING_THRESHOLD
        && tracker.last_delta_tokens < DIMINISHING_THRESHOLD;

    if !is_diminishing && (turn_tokens as f64) < (budget as f64 * COMPLETION_THRESHOLD) {
        tracker.continuation_count += 1;
        tracker.last_delta_tokens = delta_since_last;
        tracker.last_global_turn_tokens = global_turn_tokens;
        return TokenBudgetDecision::Continue {
            nudge_message: format!(
                "Token budget at {}% ({}/{}) 鈥?continue working.",
                pct, turn_tokens, budget
            ),
            continuation_count: tracker.continuation_count,
            pct,
            turn_tokens,
            budget,
        };
    }

    if is_diminishing || tracker.continuation_count > 0 {
        let now = chrono::Utc::now().timestamp_millis();
        return TokenBudgetDecision::Stop {
            completion_event: Some(BudgetCompletionEvent {
                continuation_count: tracker.continuation_count,
                pct,
                turn_tokens,
                budget,
                diminishing_returns: is_diminishing,
                duration_ms: (now - tracker.started_at) as u64,
            }),
        };
    }

    TokenBudgetDecision::Stop {
        completion_event: None,
    }
}
