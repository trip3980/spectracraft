import React, { useRef } from 'react';
import { Minimize2, Move, X } from 'lucide-react';
import type { FloatingBounds } from '../lib/workspaceLayout.ts';

interface FloatingPanelWindowProps {
  title: string;
  bounds: FloatingBounds;
  onChangeBounds: (patch: Partial<FloatingBounds>) => void;
  onRedock: () => void;
  onClose?: () => void;
  children: React.ReactNode;
}

/**
 * ArtCraft-style undocked floating studio panel (draggable title bar + corner resize).
 * Purely visual chrome — never resets or alters audio, rig, or catalog state.
 */
export function FloatingPanelWindow({
  title,
  bounds,
  onChangeBounds,
  onRedock,
  onClose,
  children,
}: FloatingPanelWindowProps) {
  const dragRef = useRef<
    | { kind: 'move'; startX: number; startY: number; origX: number; origY: number }
    | { kind: 'resize'; startX: number; startY: number; origW: number; origH: number }
    | null
  >(null);

  const handleTitleDown = (e: React.PointerEvent<HTMLDivElement>) => {
    if ((e.target as HTMLElement).closest('button')) return;
    e.currentTarget.setPointerCapture(e.pointerId);
    dragRef.current = {
      kind: 'move',
      startX: e.clientX,
      startY: e.clientY,
      origX: bounds.x,
      origY: bounds.y,
    };
  };

  const handleResizeDown = (e: React.PointerEvent<HTMLDivElement>) => {
    e.stopPropagation();
    e.currentTarget.setPointerCapture(e.pointerId);
    dragRef.current = {
      kind: 'resize',
      startX: e.clientX,
      startY: e.clientY,
      origW: bounds.w,
      origH: bounds.h,
    };
  };

  const handlePointerMove = (e: React.PointerEvent<HTMLDivElement>) => {
    const d = dragRef.current;
    if (!d) return;
    if (d.kind === 'move') {
      onChangeBounds({
        x: d.origX + (e.clientX - d.startX),
        y: d.origY + (e.clientY - d.startY),
      });
    } else {
      onChangeBounds({
        w: d.origW + (e.clientX - d.startX),
        h: d.origH + (e.clientY - d.startY),
      });
    }
  };

  const handlePointerUp = () => {
    dragRef.current = null;
  };

  return (
    <div
      style={{
        left: `${bounds.x}px`,
        top: `${bounds.y}px`,
        width: `${bounds.w}px`,
        height: `${bounds.h}px`,
      }}
      onPointerMove={handlePointerMove}
      onPointerUp={handlePointerUp}
      className="fixed z-40 bg-[#262626] border border-[#4a4a4a] shadow-2xl flex flex-col min-h-0"
    >
      <div
        onPointerDown={handleTitleDown}
        className="h-7 px-2 bg-[#2e2e2e] border-b border-[#3e3e3e] flex items-center justify-between cursor-move shrink-0 select-none"
      >
        <span className="flex items-center gap-1.5 text-[11px] font-medium text-[#e0e0e0] truncate">
          <Move className="w-3 h-3 text-[#8a8a8a] shrink-0" />
          <span>{title}</span>
        </span>
        <div className="flex items-center gap-1 shrink-0">
          <button
            type="button"
            onClick={onRedock}
            title="Re-dock panel into workspace"
            className="px-1.5 py-0.5 text-[10px] bg-[#363636] hover:bg-[#378ef0] hover:text-white text-[#cfcfcf] flex items-center gap-1"
          >
            <Minimize2 className="w-2.5 h-2.5" />
            <span>Dock</span>
          </button>
          {onClose && (
            <button
              type="button"
              onClick={onClose}
              title="Hide panel"
              className="p-0.5 text-[#9a9a9a] hover:text-white"
            >
              <X className="w-3.5 h-3.5" />
            </button>
          )}
        </div>
      </div>

      <div className="flex-1 min-h-0 flex flex-col overflow-hidden">{children}</div>

      <div
        onPointerDown={handleResizeDown}
        title="Drag to resize floating panel"
        className="absolute right-0 bottom-0 w-3.5 h-3.5 cursor-nwse-resize"
        style={{
          background:
            'linear-gradient(135deg, transparent 50%, #555555 50%, #555555 62%, transparent 62%, transparent 74%, #6a6a6a 74%)',
        }}
      />
    </div>
  );
}
