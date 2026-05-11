"use client";

import { useEffect, useState } from "react";
import { Minus, Plus } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";

interface NumberStepperProps {
  value: number;
  min?: number;
  step?: number;
  disabled?: boolean;
  className?: string;
  ariaLabel?: string;
  onCommit: (value: number) => void;
}

function normalizeNumber(value: number, min: number): number {
  if (!Number.isFinite(value)) return min;
  return Math.max(min, Math.trunc(value));
}

export function NumberStepper({
  value,
  min = 0,
  step = 1,
  disabled = false,
  className,
  ariaLabel,
  onCommit,
}: NumberStepperProps) {
  const [draft, setDraft] = useState(String(normalizeNumber(value, min)));

  useEffect(() => {
    setDraft(String(normalizeNumber(value, min)));
  }, [min, value]);

  useEffect(() => {
    if (disabled) return;
    const numeric = Number(draft);
    if (!Number.isFinite(numeric)) return;
    const next = normalizeNumber(numeric, min);
    const timer = window.setTimeout(() => {
      if (next !== value) onCommit(next);
    }, 700);
    return () => window.clearTimeout(timer);
  }, [disabled, draft, min, onCommit, value]);

  const commit = (nextValue: number) => {
    const next = normalizeNumber(nextValue, min);
    setDraft(String(next));
    if (!disabled && next !== value) {
      onCommit(next);
    }
  };

  return (
    <div
      className={cn("inline-flex h-8 w-[112px] items-center rounded-lg border border-input bg-background/45", className)}
      onWheel={(event) => {
        if (disabled) return;
        event.preventDefault();
        commit(Number(draft || value) + (event.deltaY > 0 ? -step : step));
      }}
    >
      <Button
        type="button"
        variant="ghost"
        size="icon-xs"
        className="h-7 w-7 rounded-r-none"
        disabled={disabled}
        aria-label="decrease"
        onClick={() => commit(Number(draft || value) - step)}
      >
        <Minus className="h-3 w-3" />
      </Button>
      <Input
        aria-label={ariaLabel}
        inputMode="numeric"
        className="h-7 rounded-none border-0 bg-transparent px-1 text-center font-mono text-xs focus-visible:ring-0"
        value={draft}
        disabled={disabled}
        onChange={(event) => setDraft(event.target.value)}
        onBlur={() => commit(Number(draft))}
      />
      <Button
        type="button"
        variant="ghost"
        size="icon-xs"
        className="h-7 w-7 rounded-l-none"
        disabled={disabled}
        aria-label="increase"
        onClick={() => commit(Number(draft || value) + step)}
      >
        <Plus className="h-3 w-3" />
      </Button>
    </div>
  );
}
