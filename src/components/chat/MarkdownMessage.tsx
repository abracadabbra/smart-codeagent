import { useState, useCallback, createContext, useContext } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { Prism as SyntaxHighlighter } from "react-syntax-highlighter";
import { oneDark } from "react-syntax-highlighter/dist/esm/styles/prism";

interface MarkdownMessageProps {
  content: string;
}

interface MarkdownContextValue {
  insideTable: boolean;
  insideList: boolean;
}

const MarkdownContext = createContext<MarkdownContextValue>({
  insideTable: false,
  insideList: false,
});

const codeTheme = {
  ...oneDark,
  'pre[class*="language-"]': {
    ...oneDark['pre[class*="language-"]'],
    background: "transparent",
    margin: 0,
    fontSize: "13px",
    lineHeight: "1.55",
  },
  'code[class*="language-"]': {
    ...oneDark['code[class*="language-"]'],
    background: "transparent",
    fontFamily: 'ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas, monospace',
  },
};

function InlineCode({ children }: { children?: React.ReactNode }) {
  return (
    <code className="inline-block px-1.5 py-0.5 rounded-md bg-ink-800/70 text-[#e6edf3] text-[0.85em] font-mono border border-ink-700/40 align-text-bottom">
      {children}
    </code>
  );
}

function CodeBlock({
  inline,
  className,
  children,
}: {
  inline?: boolean;
  className?: string;
  children?: React.ReactNode;
}) {
  const ctx = useContext(MarkdownContext);
  const [copied, setCopied] = useState(false);
  const match = /language-(\w+)/.exec(className || "");
  const language = match ? match[1] : "";
  const code = String(children ?? "").replace(/\n$/, "");

  if (inline || ctx.insideTable || (ctx.insideList && !code.includes("\n"))) {
    return <InlineCode>{children}</InlineCode>;
  }

  const handleCopy = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(code);
      setCopied(true);
      setTimeout(() => setCopied(false), 1800);
    } catch { /* ignore */ }
  }, [code]);

  return (
    <div className="my-3 rounded-lg overflow-hidden border border-ink-800/50 bg-[#161b22]">
      <div className="flex items-center justify-between px-3.5 py-1.5 bg-ink-800/40 border-b border-ink-800/50 select-none">
        <span className="text-[11px] text-ink-400 font-medium tracking-wide">{language || "code"}</span>
        <button
          type="button"
          onClick={handleCopy}
          className="flex items-center gap-1 text-[11px] px-2 py-0.5 rounded text-ink-400 hover:text-ink-200 hover:bg-ink-700/50 transition-colors"
        >
          {copied ? (
            <>
              <svg className="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
                <polyline points="20 6 9 17 4 12" />
              </svg>
              Copied
            </>
          ) : (
            <>
              <svg className="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <rect x="9" y="9" width="13" height="13" rx="2" ry="2" />
                <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
              </svg>
              Copy
            </>
          )}
        </button>
      </div>
      <SyntaxHighlighter
        style={codeTheme as any}
        language={language || "text"}
        PreTag="div"
        customStyle={{
          background: "#161b22",
          padding: "12px 16px",
          margin: 0,
        }}
      >
        {code}
      </SyntaxHighlighter>
    </div>
  );
}

export function MarkdownMessage({ content }: MarkdownMessageProps) {
  return (
    <div className="markdown-body max-w-none selection-brand text-[15px] leading-[1.7] text-ink-100">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        components={{
          code({ inline, className, children, ...props }: any) {
            return (
              <CodeBlock inline={inline} className={className} {...props}>
                {children}
              </CodeBlock>
            );
          },
          p({ children }) {
            return <p className="my-2 first:mt-0 last:mb-0">{children}</p>;
          },
          h1({ children }) {
            return (
              <h1 className="text-xl font-semibold mt-6 mb-3 text-ink-50 border-b border-ink-800/60 pb-2">
                {children}
              </h1>
            );
          },
          h2({ children }) {
            return (
              <h2 className="text-lg font-semibold mt-5 mb-2.5 text-ink-50">
                {children}
              </h2>
            );
          },
          h3({ children }) {
            return (
              <h3 className="text-base font-semibold mt-4 mb-2 text-ink-100">
                {children}
              </h3>
            );
          },
          ul({ children }) {
            return (
              <MarkdownContext.Provider value={{ insideTable: false, insideList: true }}>
                <ul className="my-2 pl-5 list-disc space-y-1">
                  {children}
                </ul>
              </MarkdownContext.Provider>
            );
          },
          ol({ children }) {
            return (
              <MarkdownContext.Provider value={{ insideTable: false, insideList: true }}>
                <ol className="my-2 pl-5 list-decimal space-y-1">
                  {children}
                </ol>
              </MarkdownContext.Provider>
            );
          },
          li({ children }) {
            return <li className="pl-0.5">{children}</li>;
          },
          blockquote({ children }) {
            return (
              <blockquote className="my-3 pl-4 border-l-2 border-brand-500/50 bg-ink-800/10 py-1.5 pr-4 text-ink-300 italic rounded-r">
                {children}
              </blockquote>
            );
          },
          a({ href, children }) {
            return (
              <a
                href={href}
                target="_blank"
                rel="noopener noreferrer"
                className="text-brand-400 hover:text-brand-300 underline underline-offset-2 decoration-brand-500/40 hover:decoration-brand-400 transition-colors"
              >
                {children}
              </a>
            );
          },
          table({ children }) {
            return (
              <MarkdownContext.Provider value={{ insideTable: true, insideList: false }}>
                <div className="my-4 overflow-x-auto rounded-lg border border-ink-800/60 bg-ink-900/40">
                  <table className="w-full text-[14px] border-collapse">
                    {children}
                  </table>
                </div>
              </MarkdownContext.Provider>
            );
          },
          thead({ children }) {
            return <thead className="bg-ink-800/70">{children}</thead>;
          },
          th({ children }) {
            return (
              <th className="px-4 py-2.5 text-left font-semibold text-ink-100 border-b border-ink-800/50 whitespace-nowrap">
                {children}
              </th>
            );
          },
          td({ children }) {
            return (
              <td className="px-4 py-2.5 border-b border-ink-800/30 text-ink-200 align-top">
                {children}
              </td>
            );
          },
          tr({ children }) {
            return <tr className="hover:bg-ink-800/20 transition-colors">{children}</tr>;
          },
          hr() {
            return <hr className="my-5 border-ink-800/50" />;
          },
          img({ src, alt }) {
            return (
              <img
                src={src ?? ""}
                alt={alt ?? ""}
                className="max-w-full rounded-lg my-3 border border-ink-800/50"
              />
            );
          },
          strong({ children }) {
            return <strong className="font-semibold text-ink-50">{children}</strong>;
          },
          em({ children }) {
            return <em className="italic text-ink-300">{children}</em>;
          },
        }}
      >
        {content}
      </ReactMarkdown>
    </div>
  );
}
