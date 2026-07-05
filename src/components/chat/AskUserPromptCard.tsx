import { useState } from "react";
import { useAgentStore } from "@/stores/agentStore";
import { answerAskUser } from "@/hooks/useAgentEvents";
import type { AskUserAnswer, AskUserQuestion } from "@/types/tool";

interface AnswerState {
  selectedOptionIds: string[];
  customText: string;
}

function blankAnswer(): AnswerState {
  return { selectedOptionIds: [], customText: "" };
}

/**
 * 用户提问卡片：渲染 AskUserPromptPayload（title + questions）。
 * 每个问题支持单选 / 多选 / 自定义输入。
 * 提交时把答案收齐，调 answerAskUser 命令回传。
 *
 * 借 Kivio `AskUserBlock.tsx` 的 inline form 交互。
 */
export function AskUserPromptCard() {
  const req = useAgentStore((s) => s.askUserPrompt);
  const clearAskUser = useAgentStore((s) => s.clearAskUser);
  const [answers, setAnswers] = useState<Record<string, AnswerState>>({});
  const [sending, setSending] = useState(false);

  if (!req) return null;

  const questions = req.prompt.questions ?? [];

  const getAnswer = (q: AskUserQuestion): AnswerState =>
    answers[q.id] ?? blankAnswer();

  const toggleOption = (q: AskUserQuestion, optionId: string) => {
    const cur = getAnswer(q);
    let next: string[];
    if (q.allowMultiple) {
      next = cur.selectedOptionIds.includes(optionId)
        ? cur.selectedOptionIds.filter((id) => id !== optionId)
        : [...cur.selectedOptionIds, optionId];
    } else {
      next = cur.selectedOptionIds.includes(optionId) ? [] : [optionId];
    }
    setAnswers((s) => ({ ...s, [q.id]: { ...cur, selectedOptionIds: next } }));
  };

  const setCustom = (q: AskUserQuestion, text: string) => {
    const cur = getAnswer(q);
    setAnswers((s) => ({ ...s, [q.id]: { ...cur, customText: text } }));
  };

  const allAnswered = questions.every((q) => {
    const a = getAnswer(q);
    return a.selectedOptionIds.length > 0 || a.customText.trim().length > 0;
  });

  const onSubmit = async () => {
    if (sending || !allAnswered) return;
    setSending(true);
    const payload: Record<string, AskUserAnswer> = {};
    for (const q of questions) {
      const a = getAnswer(q);
      payload[q.id] = {
        selectedOptionIds: a.selectedOptionIds,
        customText: a.customText.trim() || undefined,
      };
    }
    try {
      await answerAskUser(req.askUserId, "answered", payload);
    } catch (err) {
      // eslint-disable-next-line no-console
      console.error("answerAskUser failed:", err);
    } finally {
      setSending(false);
      clearAskUser();
    }
  };

  return (
    <div className="rounded-lg border border-blue-500/40 bg-blue-950/20 p-3 text-xs">
      <div className="font-medium text-blue-200 mb-2">
        {req.prompt.title ?? "Agent 提问"}
      </div>

      <div className="space-y-3">
        {questions.map((q) => {
          const a = getAnswer(q);
          return (
            <div key={q.id} className="space-y-1.5">
              <div className="text-ink-100">{q.prompt}</div>
              <div className="flex flex-col gap-1">
                {q.options.map((opt) => {
                  const checked = a.selectedOptionIds.includes(opt.id);
                  return (
                    <label
                      key={opt.id}
                      className="flex items-start gap-2 cursor-pointer rounded px-2 py-1 hover:bg-ink-700/40"
                    >
                      <input
                        type={q.allowMultiple ? "checkbox" : "radio"}
                        name={`q-${q.id}`}
                        checked={checked}
                        onChange={() => toggleOption(q, opt.id)}
                        className="mt-0.5 accent-blue-500"
                      />
                      <span>
                        <span className="text-ink-100">{opt.label}</span>
                        {opt.description && (
                          <span className="block text-ink-400 text-[11px]">
                            {opt.description}
                          </span>
                        )}
                      </span>
                    </label>
                  );
                })}
              </div>
              {q.allowCustom && (
                <input
                  type="text"
                  value={a.customText}
                  onChange={(e) => setCustom(q, e.target.value)}
                  placeholder="自定义回答…"
                  className="w-full rounded bg-ink-900/80 px-2 py-1 text-ink-100 placeholder:text-ink-400 outline-none border border-ink-600 focus:border-blue-500 text-[11px]"
                />
              )}
            </div>
          );
        })}
      </div>

      <div className="mt-3 flex justify-end">
        <button
          type="button"
          disabled={!allAnswered || sending}
          onClick={() => void onSubmit()}
          className="px-3 py-1.5 rounded-lg bg-blue-600 hover:bg-blue-500 disabled:bg-ink-600 disabled:cursor-not-allowed text-white text-sm font-medium transition-colors"
        >
          {sending ? "提交中…" : "提交回答"}
        </button>
      </div>
    </div>
  );
}
