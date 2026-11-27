import { useEffect, useMemo, useRef, useState } from 'react';
import { ArrowRight, BookOpen, HelpCircle, Info, Search, X } from 'lucide-react';
import {
  EMBEDDED_DOCS,
  HELP_CATEGORIES,
  HELP_TOPICS,
  searchHelpTopics,
  type EmbeddedDocMeta,
  type HelpCategory,
  type HelpNavigationAction,
  type HelpTopic,
} from '../lib/helpCatalog.ts';
import { getTransport } from '../lib/transport.ts';

export type HelpModalTab = 'search' | 'glossary' | 'docs' | 'about';

interface HelpModalProps {
  open: boolean;
  initialTab: HelpModalTab;
  initialQuery?: string;
  initialDocId?: EmbeddedDocMeta['id'];
  onClose: () => void;
  onNavigateAction: (action: HelpNavigationAction) => void;
}

interface LoadedDoc {
  id: string;
  title: string;
  path: string;
  content: string;
}

export function HelpModal({
  open,
  initialTab,
  initialQuery = '',
  initialDocId = 'readme',
  onClose,
  onNavigateAction,
}: HelpModalProps) {
  const [activeTab, setActiveTab] = useState<HelpModalTab>(initialTab);
  const [query, setQuery] = useState(initialQuery);
  const [selectedCategory, setSelectedCategory] = useState<HelpCategory | 'All'>('All');
  const [selectedDocId, setSelectedDocId] = useState<EmbeddedDocMeta['id']>(initialDocId);
  const [docCache, setDocCache] = useState<Record<string, LoadedDoc>>({});
  const [docLoading, setDocLoading] = useState(false);
  const [docError, setDocError] = useState<string | null>(null);
  const searchInputRef = useRef<HTMLInputElement | null>(null);

  useEffect(() => {
    if (!open) return;
    setActiveTab(initialTab);
    if (initialQuery) setQuery(initialQuery);
    if (initialDocId) setSelectedDocId(initialDocId);
  }, [open, initialTab, initialQuery, initialDocId]);

  useEffect(() => {
    if (open && activeTab === 'search') {
      const timer = window.setTimeout(() => {
        searchInputRef.current?.focus();
      }, 20);
      return () => window.clearTimeout(timer);
    }
  }, [open, activeTab]);

  useEffect(() => {
    if (!open || activeTab !== 'docs') return;
    if (docCache[selectedDocId]) {
      setDocError(null);
      return;
    }
    let active = true;
    const ctrl = new AbortController();
    setDocLoading(true);
    setDocError(null);
    getTransport()
      .getHelpDoc(selectedDocId, ctrl.signal)
      .then((doc) => {
        if (active) {
          setDocCache((prev) => ({ ...prev, [selectedDocId]: doc }));
        }
      })
      .catch((err) => {
        if (active && !(err instanceof DOMException && err.name === 'AbortError')) {
          setDocError(err instanceof Error ? err.message : String(err));
        }
      })
      .finally(() => {
        if (active) setDocLoading(false);
      });
    return () => {
      active = false;
      ctrl.abort();
    };
  }, [open, activeTab, selectedDocId, docCache]);

  const searchResults = useMemo(() => searchHelpTopics(query), [query]);

  const glossaryTopics = useMemo(
    () =>
      selectedCategory === 'All'
        ? HELP_TOPICS
        : HELP_TOPICS.filter((t) => t.category === selectedCategory),
    [selectedCategory],
  );

  const handleSelectTopic = (topic: HelpTopic) => {
    onClose();
    onNavigateAction(topic.action);
  };

  if (!open) return null;

  const activeDoc = docCache[selectedDocId] ?? null;

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-label="SpectraCraft Help System"
      data-testid="help-modal"
      className="fixed inset-0 z-50 bg-black/75 flex items-center justify-center p-4"
      onClick={onClose}
    >
      <div
        className="w-full max-w-[920px] h-[84vh] bg-[#14171f] border border-[#2f394c] shadow-2xl flex flex-col text-[#e5ecf7]"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Header */}
        <div className="px-4 py-3 bg-[#1a1f2b] border-b border-[#2c3547] flex items-center justify-between gap-3 shrink-0">
          <div className="min-w-0">
            <div className="flex items-center gap-2">
              <HelpCircle className="w-4 h-4 text-[#5bb6ff] shrink-0" />
              <h2 className="text-[14px] font-semibold tracking-tight text-[#f4f7fc]">
                SpectraCraft Help
              </h2>
              <span className="font-mono text-[11px] text-[#8d9ab0]">
                Search · Glossary · Documentation · About
              </span>
            </div>
          </div>

          <button
            type="button"
            onClick={onClose}
            aria-label="Close Help"
            className="h-7 w-7 bg-[#1f2634] hover:bg-[#2a3345] border border-[#38455c] flex items-center justify-center text-[#b7c4d9]"
          >
            <X className="w-4 h-4" />
          </button>
        </div>

        {/* Navigation Tabs */}
        <div className="px-4 py-2 bg-[#12161f] border-b border-[#252d3c] flex items-center gap-1.5 shrink-0">
          {(
            [
              ['search', 'Search & Jump to Tool', Search],
              ['glossary', 'Glossary & Definitions', HelpCircle],
              ['docs', 'READMEs & Technical Notes', BookOpen],
              ['about', 'About SpectraCraft', Info],
            ] as const
          ).map(([id, label, Icon]) => (
            <button
              key={id}
              type="button"
              data-testid={`help-tab-${id}`}
              onClick={() => setActiveTab(id)}
              className={`px-3 py-1.5 text-[11.5px] border flex items-center gap-1.5 whitespace-nowrap transition-colors ${
                activeTab === id
                  ? 'bg-[#213652] border-[#3d78c2] text-[#f4f8ff] font-medium'
                  : 'bg-[#171c26] border-[#2a3345] text-[#9aa8be] hover:text-[#e6edf8]'
              }`}
            >
              <Icon className="w-3.5 h-3.5" />
              <span>{label}</span>
            </button>
          ))}
        </div>

        {/* Tab Body */}
        <div className="flex-1 min-h-0 flex flex-col overflow-hidden">
          {activeTab === 'search' && (
            <div className="flex-1 min-h-0 flex flex-col p-4 gap-3">
              <div className="flex items-center gap-2 bg-[#10141c] border border-[#36445c] px-3 h-9 shrink-0 focus-within:border-[#4b90e2]">
                <Search className="w-4 h-4 text-[#7c94b8] shrink-0" />
                <input
                  ref={searchInputRef}
                  type="text"
                  data-testid="help-search-input"
                  value={query}
                  onChange={(e) => setQuery(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter' && searchResults.length > 0) {
                      e.preventDefault();
                      handleSelectTopic(searchResults[0]);
                    }
                  }}
                  placeholder="Type a control, stage, diagnostic, or term (e.g. report, craft-pdf, mic, marquee, update)…"
                  className="w-full bg-transparent text-[12.5px] text-[#f0f4fb] placeholder-[#738099] focus:outline-none"
                />
                {query && (
                  <button
                    type="button"
                    onClick={() => setQuery('')}
                    className="text-[11px] font-mono text-[#8d9ab0] hover:text-white"
                  >
                    Clear
                  </button>
                )}
              </div>

              <div className="flex items-center justify-between text-[11px] font-mono text-[#8b98ae] shrink-0">
                <span>
                  {searchResults.length} matching topic{searchResults.length === 1 ? '' : 's'}
                </span>
                <span>Select any result or press Enter to jump directly to the tool</span>
              </div>

              <div className="flex-1 overflow-y-auto divide-y divide-[#232a38] border border-[#2a3242] bg-[#161a23]">
                {searchResults.length === 0 ? (
                  <div className="p-8 text-center text-[12px] text-[#8d99ad]">
                    No implemented SpectraCraft topic matched “{query}”. Try searching for
                    “stage 1”, “marquee”, “microphone”, “room”, “report”, “craft-pdf”, or “updates”.
                  </div>
                ) : (
                  searchResults.map((topic) => (
                    <div
                      key={topic.id}
                      data-testid={`help-result-${topic.id}`}
                      className="px-3.5 py-2.5 hover:bg-[#1c2330] flex items-start justify-between gap-4 transition-colors"
                    >
                      <div className="min-w-0 space-y-1">
                        <div className="flex flex-wrap items-center gap-2">
                          <span className="text-[12.5px] font-semibold text-[#eef3fb]">
                            {topic.title}
                          </span>
                          <span className="font-mono text-[10.5px] text-[#7ea3d1]">
                            {topic.category}
                          </span>
                          {topic.shortcut && (
                            <span className="font-mono text-[10px] text-[#9eb0cc]">
                              · {topic.shortcut}
                            </span>
                          )}
                        </div>
                        <div className="font-mono text-[10.5px] text-[#7d8ca6]">
                          {topic.menuPath}
                        </div>
                        <p className="text-[11.5px] text-[#c2cde0] leading-relaxed">
                          {topic.definition}
                        </p>
                      </div>

                      <button
                        type="button"
                        data-testid={`help-go-${topic.id}`}
                        onClick={() => handleSelectTopic(topic)}
                        className="h-7 px-3 bg-[#245aa3] hover:bg-[#2e6ec4] border border-[#4284dc] text-white text-[11px] font-medium flex items-center gap-1.5 whitespace-nowrap shrink-0 mt-0.5"
                      >
                        <span>{topic.actionLabel}</span>
                        <ArrowRight className="w-3.5 h-3.5" />
                      </button>
                    </div>
                  ))
                )}
              </div>
            </div>
          )}

          {activeTab === 'glossary' && (
            <div className="flex-1 min-h-0 flex flex-col p-4 gap-3">
              <div className="flex flex-wrap items-center gap-1.5 shrink-0">
                {(['All', ...HELP_CATEGORIES] as const).map((cat) => (
                  <button
                    key={cat}
                    type="button"
                    onClick={() => setSelectedCategory(cat)}
                    className={`px-2.5 py-1 text-[11px] border whitespace-nowrap ${
                      selectedCategory === cat
                        ? 'bg-[#213652] border-[#3d78c2] text-white font-medium'
                        : 'bg-[#171c26] border-[#2a3345] text-[#9aa8be] hover:text-[#e6edf8]'
                    }`}
                  >
                    {cat}
                  </button>
                ))}
              </div>

              <div className="flex-1 overflow-y-auto divide-y divide-[#232a38] border border-[#2a3242] bg-[#161a23]">
                {glossaryTopics.map((topic) => (
                  <div
                    key={topic.id}
                    className="px-4 py-3 flex items-start justify-between gap-4"
                  >
                    <div className="space-y-1 min-w-0">
                      <div className="flex flex-wrap items-baseline gap-2">
                        <h3 className="text-[12.5px] font-semibold text-[#f1f5fc]">
                          {topic.title}
                        </h3>
                        <span className="font-mono text-[10.5px] text-[#8391a8]">
                          {topic.menuPath}
                        </span>
                      </div>
                      <p className="text-[11.5px] text-[#c5d0e3] leading-relaxed">
                        {topic.definition}
                      </p>
                    </div>
                    <button
                      type="button"
                      onClick={() => handleSelectTopic(topic)}
                      className="h-7 px-2.5 bg-[#1e2838] hover:bg-[#2a3a52] border border-[#3a4f70] text-[#d7e5fa] text-[11px] whitespace-nowrap shrink-0"
                    >
                      {topic.actionLabel}
                    </button>
                  </div>
                ))}
              </div>
            </div>
          )}

          {activeTab === 'docs' && (
            <div className="flex-1 min-h-0 flex">
              <aside className="w-[248px] bg-[#12161f] border-r border-[#252d3c] p-2.5 overflow-y-auto space-y-1 shrink-0">
                {EMBEDDED_DOCS.map((doc) => (
                  <button
                    key={doc.id}
                    type="button"
                    data-testid={`help-doc-${doc.id}`}
                    onClick={() => setSelectedDocId(doc.id)}
                    className={`w-full text-left px-2.5 py-2 border transition-colors ${
                      selectedDocId === doc.id
                        ? 'bg-[#1e314a] border-[#3d78c2] text-[#f2f7ff]'
                        : 'bg-[#161b25] border-[#262f40] text-[#b4c1d6] hover:bg-[#1c2331]'
                    }`}
                  >
                    <div className="text-[11.5px] font-medium">{doc.title}</div>
                    <div className="font-mono text-[10px] text-[#7f8da6] mt-0.5 truncate">
                      {doc.filename}
                    </div>
                  </button>
                ))}
              </aside>

              <div className="flex-1 min-w-0 flex flex-col bg-[#10141c]">
                <div className="px-4 py-2 bg-[#161b25] border-b border-[#252d3c] flex items-center justify-between text-[11px] font-mono text-[#94a3bc] shrink-0">
                  <span>{activeDoc?.title ?? selectedDocId}</span>
                  <span>{activeDoc?.path ?? ''}</span>
                </div>
                <div className="flex-1 overflow-y-auto p-4">
                  {docLoading && !activeDoc && (
                    <div className="text-[12px] font-mono text-[#8d9ab0]">
                      Loading documentation…
                    </div>
                  )}
                  {docError && (
                    <div className="p-3 bg-[#2d1619] border border-[#7a2e38] text-[#ffb4be] font-mono text-[11.5px]">
                      {docError}
                    </div>
                  )}
                  {activeDoc && (
                    <pre
                      data-testid="help-doc-content"
                      className="font-mono text-[11.5px] leading-relaxed text-[#dce5f4] whitespace-pre-wrap break-words select-text"
                    >
                      {activeDoc.content}
                    </pre>
                  )}
                </div>
              </div>
            </div>
          )}

          {activeTab === 'about' && (
            <div className="flex-1 overflow-y-auto p-5 space-y-5 text-[12px] leading-relaxed">
              <section className="space-y-2 max-w-3xl">
                <h3 className="text-[16px] font-semibold text-[#f4f7fc]">SpectraCraft</h3>
                <p className="text-[#cad5e8]">
                  SpectraCraft is a single-source native Rust audio workstation that extracts
                  performance cues from an isolated instrument, voice, or stem and synthesizes a
                  completely fresh dry take, then lets you apply localized dry repairs and optional
                  microphone, room, and finishing stages.
                </p>
              </section>

              <section className="border-t border-[#262f40] pt-4 space-y-2 max-w-3xl">
                <h4 className="text-[12.5px] font-semibold text-[#eef3fb]">
                  Fixed Five-Stage Pipeline
                </h4>
                <ol className="list-decimal list-inside space-y-1 text-[#c2cde0] font-mono text-[11.5px]">
                  <li>Stage 1 — Dry Regeneration (persistent modal physics; fresh samples only)</li>
                  <li>Stage 2 — Surgical Region Editing (targets cached dry take only)</li>
                  <li>Stage 3 — Optional Microphone (bypassed by default)</li>
                  <li>Stage 4 — Optional Space / Room (bypassed by default)</li>
                  <li>Stage 5 — Optional Plugin Slot (bypassed by default)</li>
                </ol>
              </section>

              <section className="border-t border-[#262f40] pt-4 space-y-2 max-w-3xl">
                <h4 className="text-[12.5px] font-semibold text-[#eef3fb]">
                  Core Guarantees &amp; Pinned ArtCraft Libraries
                </h4>
                <ul className="list-disc list-inside space-y-1 text-[#c2cde0]">
                  <li>
                    <strong>Fresh-signal invariant:</strong> original raw PCM never enters regenerated
                    or captured outputs.
                  </li>
                  <li>
                    <strong>Blind Before/After reporting:</strong> raw input and final export are
                    analyzed independently, displayed in the dark-mode viewer, and exported via pinned{' '}
                    <span className="font-mono text-[#dce6f5]">craft-pdf</span> and{' '}
                    <span className="font-mono text-[#dce6f5]">craft-atomic</span>.
                  </li>
                  <li>
                    <strong>ArtCraft settings &amp; controlled updates:</strong> local uncommitted{' '}
                    <span className="font-mono text-[#dce6f5]">config.toml</span> managed via pinned{' '}
                    <span className="font-mono text-[#dce6f5]">craft-settings</span>, with opt-in
                    background update checks gated by the mandatory 20 Hz–20 kHz sweep test.
                  </li>
                </ul>
              </section>

              <div className="border-t border-[#262f40] pt-4 flex flex-wrap items-center gap-2">
                <button
                  type="button"
                  onClick={() => setActiveTab('docs')}
                  className="h-7 px-3 bg-[#1e2a3c] hover:bg-[#283952] border border-[#3a5275] text-[#dce8fa] text-[11.5px]"
                >
                  Browse READMEs &amp; Technical Notes
                </button>
                <button
                  type="button"
                  onClick={() => {
                    onClose();
                    onNavigateAction('settings-modal');
                  }}
                  className="h-7 px-3 bg-[#1e2a3c] hover:bg-[#283952] border border-[#3a5275] text-[#dce8fa] text-[11.5px]"
                >
                  Open Settings &amp; Updates
                </button>
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
