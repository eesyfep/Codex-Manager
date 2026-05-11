"use client";

import { useEffect, useMemo, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";
import { Check, ChevronDown, Search } from "lucide-react";

export interface SearchableModelOption {
  value: string;
  label: string;
  keywords?: string[];
}

interface SearchableModelPickerProps {
  value: string;
  onValueChange: (value: string) => void;
  options: SearchableModelOption[];
  placeholder: string;
  searchPlaceholder?: string;
  emptyLabel?: string;
  disabled?: boolean;
  allowCustomValue?: boolean;
  customValuePrefix?: string;
  triggerClassName?: string;
  dropdownClassName?: string;
}

function normalizeSearchParts(option: SearchableModelOption): string {
  return [
    option.value,
    option.label,
    ...(option.keywords || []),
  ]
    .join(" ")
    .toLowerCase();
}

export function SearchableModelPicker({
  value,
  onValueChange,
  options,
  placeholder,
  searchPlaceholder = "Search models",
  emptyLabel = "No matching models",
  disabled = false,
  allowCustomValue = false,
  customValuePrefix = "Use",
  triggerClassName,
  dropdownClassName,
}: SearchableModelPickerProps) {
  const rootRef = useRef<HTMLDivElement | null>(null);
  const inputRef = useRef<HTMLInputElement | null>(null);
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");

  const normalizedOptions = useMemo(
    () =>
      options.map((option) => ({
        ...option,
        haystack: normalizeSearchParts(option),
      })),
    [options],
  );

  const selectedOption = useMemo(
    () =>
      normalizedOptions.find(
        (option) => option.value.trim() === String(value || "").trim(),
      ) || null,
    [normalizedOptions, value],
  );

  const filteredOptions = useMemo(() => {
    const keyword = query.trim().toLowerCase();
    if (!keyword) {
      return normalizedOptions;
    }
    return normalizedOptions.filter((option) => option.haystack.includes(keyword));
  }, [normalizedOptions, query]);

  const customValue = query.trim();
  const canUseCustomValue =
    allowCustomValue &&
    Boolean(customValue) &&
    !normalizedOptions.some(
      (option) => option.value.toLowerCase() === customValue.toLowerCase(),
    );

  useEffect(() => {
    if (!open) return;
    const frame = window.requestAnimationFrame(() => {
      inputRef.current?.focus();
      inputRef.current?.select();
    });

    return () => window.cancelAnimationFrame(frame);
  }, [open]);

  useEffect(() => {
    if (!open) return;

    const handlePointerDown = (event: MouseEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) {
        setOpen(false);
        setQuery("");
      }
    };

    const handleEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setOpen(false);
        setQuery("");
      }
    };

    window.addEventListener("mousedown", handlePointerDown);
    window.addEventListener("keydown", handleEscape);
    return () => {
      window.removeEventListener("mousedown", handlePointerDown);
      window.removeEventListener("keydown", handleEscape);
    };
  }, [open]);

  const currentDisplay = selectedOption?.label || value || placeholder;

  const commitValue = (nextValue: string) => {
    onValueChange(nextValue);
    setOpen(false);
    setQuery("");
  };

  return (
    <div ref={rootRef} className="relative">
      <Button
        type="button"
        variant="outline"
        className={cn(
          "w-full justify-between rounded-lg border-input bg-background/45 px-3 font-normal",
          !value && "text-muted-foreground",
          triggerClassName,
        )}
        disabled={disabled}
        onClick={() =>
          setOpen((current) => {
            const next = !current;
            if (!next) {
              setQuery("");
            }
            return next;
          })
        }
      >
        <span className="truncate text-left">{currentDisplay}</span>
        <ChevronDown className="h-4 w-4 shrink-0 text-muted-foreground" />
      </Button>

      {open ? (
        <div
          className={cn(
            "absolute z-50 mt-2 w-full rounded-lg border border-border bg-popover p-2 shadow-md ring-1 ring-foreground/10",
            dropdownClassName,
          )}
        >
          <div className="relative">
            <Search className="pointer-events-none absolute left-2.5 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
            <Input
              ref={inputRef}
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  event.preventDefault();
                  if (filteredOptions.length > 0) {
                    commitValue(filteredOptions[0].value);
                    return;
                  }
                  if (canUseCustomValue) {
                    commitValue(customValue);
                  }
                }
              }}
              placeholder={searchPlaceholder}
              className="pl-8"
            />
          </div>

          <div className="mt-2 max-h-64 overflow-y-auto">
            {canUseCustomValue ? (
              <button
                type="button"
                className="flex w-full items-center justify-between rounded-md px-2 py-2 text-left text-sm hover:bg-accent hover:text-accent-foreground"
                onClick={() => commitValue(customValue)}
              >
                <span className="truncate">
                  {customValuePrefix} <span className="font-mono">{customValue}</span>
                </span>
              </button>
            ) : null}

            {filteredOptions.length > 0 ? (
              filteredOptions.map((option) => {
                const isSelected = option.value === value;
                return (
                  <button
                    key={option.value}
                    type="button"
                    className={cn(
                      "flex w-full items-center justify-between gap-3 rounded-md px-2 py-2 text-left text-sm hover:bg-accent hover:text-accent-foreground",
                      isSelected && "bg-accent/60",
                    )}
                    onClick={() => commitValue(option.value)}
                  >
                    <span className="min-w-0">
                      <span className="block truncate">{option.label}</span>
                      {option.label !== option.value ? (
                        <span className="block truncate font-mono text-[11px] text-muted-foreground">
                          {option.value}
                        </span>
                      ) : null}
                    </span>
                    {isSelected ? <Check className="h-4 w-4 shrink-0" /> : null}
                  </button>
                );
              })
            ) : (
              <div className="px-2 py-3 text-sm text-muted-foreground">
                {emptyLabel}
              </div>
            )}
          </div>
        </div>
      ) : null}
    </div>
  );
}
