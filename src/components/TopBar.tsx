import React from 'react';

export type WorkspaceTab = 'dry' | 'cues' | 'layers' | 'variants' | 'audit';

interface TopBarProps {
  activeTab: WorkspaceTab;
  onSelectTab: (tab: WorkspaceTab) => void;
  onTriggerUpload: () => void;
  uploading: boolean;
}

const NAV_ITEMS: ReadonlyArray<{ id: WorkspaceTab; label: string }> = [
  { id: 'dry', label: 'Dry Rebuild' },
  { id: 'cues', label: 'Cue Editor' },
  { id: 'layers', label: 'Layers & Repair' },
  { id: 'variants', label: 'Variants' },
  { id: 'audit', label: 'Audit & Spec' },
];

export function TopBar({ activeTab, onSelectTab, onTriggerUpload, uploading }: TopBarProps) {
  return (
    <header className="sticky top-0 z-30 border-b border-slate-200 bg-white/95 backdrop-blur-sm">
      <div className="mx-auto flex max-w-[1440px] items-center justify-between gap-8 px-6 py-3.5">
        <a
          href="#dry"
          onClick={(e) => {
            e.preventDefault();
            onSelectTab('dry');
          }}
          className="shrink-0 whitespace-nowrap text-lg font-bold tracking-tight text-slate-900"
        >
          SpectraCraft
        </a>

        <nav aria-label="Workspace sections" className="flex items-center gap-6 overflow-x-auto">
          {NAV_ITEMS.map((item) => {
            const active = activeTab === item.id;
            return (
              <button
                key={item.id}
                type="button"
                aria-current={active ? 'page' : undefined}
                onClick={() => onSelectTab(item.id)}
                className={`shrink-0 whitespace-nowrap border-b-2 py-1 text-sm font-medium transition-colors ${
                  active
                    ? 'border-blue-700 text-slate-900'
                    : 'border-transparent text-slate-600 hover:border-slate-300 hover:text-slate-900'
                }`}
              >
                {item.label}
              </button>
            );
          })}
        </nav>

        <div className="shrink-0">
          <button
            type="button"
            disabled={uploading}
            onClick={onTriggerUpload}
            className="shrink-0 whitespace-nowrap rounded-md bg-slate-900 px-4 py-2 text-sm font-medium text-white transition-colors hover:bg-slate-800 disabled:cursor-not-allowed disabled:opacity-60"
          >
            {uploading ? 'Analyzing WAV…' : 'Upload WAV'}
          </button>
        </div>
      </div>
    </header>
  );
}
