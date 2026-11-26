import { useEffect, useRef, useState } from 'react';
import { Check, ChevronRight } from 'lucide-react';

export type MenuEntry =
  | { kind: 'separator' }
  | { kind: 'section'; label: string }
  | {
      kind: 'item';
      id: string;
      label: string;
      shortcut?: string;
      checked?: boolean;
      disabled?: boolean;
      onSelect: () => void;
    }
  | {
      kind: 'submenu';
      id: string;
      label: string;
      disabled?: boolean;
      items: MenuEntry[];
    };

export interface TopMenuDefinition {
  id: string;
  label: string;
  items: MenuEntry[];
}

interface MenuBarProps {
  menus: TopMenuDefinition[];
  openMenuId: string | null;
  onOpenMenuChange: (id: string | null) => void;
  rightMeta: string;
  playheadReadout?: string | null;
}

function SubmenuList({
  items,
  onCloseAll,
  alignLeft = false,
  depth = 0,
}: {
  items: MenuEntry[];
  onCloseAll: () => void;
  alignLeft?: boolean;
  depth?: number;
}) {
  const [openSubId, setOpenSubId] = useState<string | null>(null);

  return (
    <div
      role="menu"
      className={`min-w-[244px] bg-[#26272c] border border-[#42454f] py-1 shadow-xl text-[12px] text-[#e2e5ec] ${
        depth > 0
          ? `absolute top-[-5px] ${alignLeft ? 'right-full mr-px' : 'left-full ml-px'} z-50`
          : 'absolute left-0 top-full mt-px z-40'
      }`}
    >
      {items.map((entry, idx) => {
        if (entry.kind === 'separator') {
          return <div key={`sep-${idx}`} className="my-1 border-t border-[#383b44]" />;
        }
        if (entry.kind === 'section') {
          return (
            <div
              key={`sec-${entry.label}-${idx}`}
              className="px-3 pt-1.5 pb-0.5 text-[10px] font-mono text-[#8a91a0] select-none"
            >
              {entry.label}
            </div>
          );
        }
        if (entry.kind === 'submenu') {
          const isOpen = openSubId === entry.id;
          return (
            <div
              key={entry.id}
              className="relative"
              onMouseEnter={() => {
                if (!entry.disabled) setOpenSubId(entry.id);
              }}
              onMouseLeave={() => {
                setOpenSubId((curr) => (curr === entry.id ? null : curr));
              }}
            >
              <button
                type="button"
                role="menuitem"
                aria-haspopup="menu"
                aria-expanded={isOpen}
                disabled={entry.disabled}
                data-testid={`menu-sub-${entry.id}`}
                onClick={(e) => {
                  e.stopPropagation();
                  if (!entry.disabled) {
                    setOpenSubId((curr) => (curr === entry.id ? null : entry.id));
                  }
                }}
                className={`w-full px-3 py-1.5 text-left flex items-center justify-between gap-4 whitespace-nowrap transition-colors disabled:text-[#666a73] disabled:hover:bg-transparent ${
                  isOpen
                    ? 'bg-[#2f6ec4] text-white'
                    : 'text-[#dfe3eb] hover:bg-[#2f6ec4] hover:text-white'
                }`}
              >
                <span className="flex items-center gap-2">
                  <span className="w-3.5 inline-flex items-center justify-center shrink-0" />
                  <span>{entry.label}</span>
                </span>
                <ChevronRight className="w-3.5 h-3.5 opacity-80 shrink-0" />
              </button>
              {isOpen && !entry.disabled && (
                <SubmenuList
                  items={entry.items}
                  onCloseAll={onCloseAll}
                  alignLeft={alignLeft}
                  depth={depth + 1}
                />
              )}
            </div>
          );
        }

        return (
          <button
            key={entry.id}
            type="button"
            role="menuitem"
            disabled={entry.disabled}
            data-testid={`menu-item-${entry.id}`}
            onMouseEnter={() => setOpenSubId(null)}
            onClick={(e) => {
              e.stopPropagation();
              if (entry.disabled) return;
              onCloseAll();
              entry.onSelect();
            }}
            className="w-full px-3 py-1.5 text-left flex items-center justify-between gap-4 whitespace-nowrap text-[#dfe3eb] hover:bg-[#2f6ec4] hover:text-white disabled:text-[#666a73] disabled:hover:bg-transparent transition-colors"
          >
            <span className="flex items-center gap-2">
              <span className="w-3.5 inline-flex items-center justify-center shrink-0">
                {entry.checked ? <Check className="w-3.5 h-3.5" /> : null}
              </span>
              <span>{entry.label}</span>
            </span>
            {entry.shortcut && (
              <span className="font-mono text-[10px] opacity-70 tabular-nums shrink-0">
                {entry.shortcut}
              </span>
            )}
          </button>
        );
      })}
    </div>
  );
}

export function MenuBar({
  menus,
  openMenuId,
  onOpenMenuChange,
  rightMeta,
  playheadReadout,
}: MenuBarProps) {
  const barRef = useRef<HTMLElement | null>(null);

  useEffect(() => {
    if (!openMenuId) return;
    const handleOutsideClick = (e: MouseEvent) => {
      if (barRef.current && !barRef.current.contains(e.target as Node)) {
        onOpenMenuChange(null);
      }
    };
    window.addEventListener('mousedown', handleOutsideClick);
    return () => window.removeEventListener('mousedown', handleOutsideClick);
  }, [openMenuId, onOpenMenuChange]);

  return (
    <header
      ref={barRef}
      data-testid="main-menu-bar"
      className="h-8 bg-[#24252a] border-b border-[#383a42] px-2.5 flex items-center justify-between gap-4 text-[12px] shrink-0 relative z-30 select-none"
      onClick={(e) => e.stopPropagation()}
    >
      <div className="flex items-center gap-0.5 min-w-0">
        <div className="flex items-center gap-2 pr-3 mr-1 border-r border-[#3c3f48] shrink-0">
          <span className="font-semibold tracking-tight text-[#f1f3f7] whitespace-nowrap">
            SpectraCraft
          </span>
        </div>

        <nav aria-label="Main menu" className="flex items-center gap-0.5">
          {menus.map((menu) => {
            const isOpen = openMenuId === menu.id;
            const alignSubLeft = menu.id === 'help';
            return (
              <div key={menu.id} className="relative">
                <button
                  type="button"
                  aria-haspopup="menu"
                  aria-expanded={isOpen}
                  data-testid={`menu-trigger-${menu.id}`}
                  onClick={() => onOpenMenuChange(isOpen ? null : menu.id)}
                  onMouseEnter={() => {
                    if (openMenuId && openMenuId !== menu.id) {
                      onOpenMenuChange(menu.id);
                    }
                  }}
                  className={`px-2.5 py-1 text-[12px] whitespace-nowrap shrink-0 transition-colors ${
                    isOpen
                      ? 'bg-[#2f6ec4] text-white'
                      : 'text-[#d4d8e0] hover:bg-[#33363f] hover:text-white'
                  }`}
                >
                  {menu.label}
                </button>

                {isOpen && (
                  <SubmenuList
                    items={menu.items}
                    onCloseAll={() => onOpenMenuChange(null)}
                    alignLeft={alignSubLeft}
                  />
                )}
              </div>
            );
          })}
        </nav>
      </div>

      {/* Quiet right-side session/time metadata — clean unboxed typography */}
      <div className="flex items-center gap-2.5 font-mono text-[11px] text-[#989fa9] tabular-nums min-w-0 shrink-0">
        {playheadReadout && (
          <>
            <span className="text-[#cfd6e2] whitespace-nowrap">{playheadReadout}</span>
            <span aria-hidden="true" className="text-[#525661]">
              ·
            </span>
          </>
        )}
        <span className="truncate max-w-[380px]">{rightMeta}</span>
      </div>
    </header>
  );
}
