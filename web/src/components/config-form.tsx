import { useEffect, useMemo, useState } from 'react';
import { cn } from '../lib/utils';
import { Alert } from './ui/alert';
import { Button } from './ui/button';
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from './ui/collapsible';
import { Input } from './ui/input';
import { Label } from './ui/label';
import { Switch } from './ui/switch';
import type { FormDoc, FormField } from '../lib/types';

type Draft = string | boolean;
type Edit = { key: string; value: unknown };

const selectClass = cn(
  'flex h-11 w-full rounded-lg border border-input bg-card px-3 py-2 text-base text-foreground outline-none',
  'focus-visible:border-ring focus-visible:ring-2 focus-visible:ring-ring/40',
);

/** What the control shows for a field before the owner touches it. */
function initialDraft(f: FormField): Draft {
  if (f.kind === 'bool') {
    return f.value === null || f.value === undefined ? f.default === true : f.value === true;
  }
  if (f.value === null || f.value === undefined) {
    return '';
  }
  if (f.kind === 'list') {
    return Array.isArray(f.value) ? f.value.map(String).join('\n') : '';
  }
  return String(f.value);
}

/** The JSON value a draft stands for, or the message to show when it cannot be one. */
function toValue(f: FormField, draft: Draft): { value: unknown } | { error: string } {
  if (f.kind === 'bool') {
    return { value: draft === true };
  }
  const text = String(draft);
  if (f.kind === 'list') {
    const items = text
      .split('\n')
      .map((l) => l.trim())
      .filter(Boolean);
    if (items.length === 0 && (f.value === null || f.value === undefined)) {
      return { value: null };
    }
    if (f.item_kind !== 'int' && f.item_kind !== 'float') {
      return { value: items };
    }
    const nums = items.map(Number);
    if (nums.some((n) => !Number.isFinite(n) || (f.item_kind === 'int' && !Number.isInteger(n)))) {
      return { error: f.item_kind === 'int' ? 'Every line must be a whole number.' : 'Every line must be a number.' };
    }
    return { value: nums };
  }
  const trimmed = f.kind === 'string' || f.kind === 'enum' ? text : text.trim();
  if (trimmed === '') {
    return { value: null };
  }
  if (f.kind === 'int' || f.kind === 'float') {
    const n = Number(trimmed);
    if (!Number.isFinite(n)) {
      return { error: f.kind === 'int' ? 'Enter a whole number.' : 'Enter a number.' };
    }
    if (f.kind === 'int' && !Number.isInteger(n)) {
      return { error: 'Enter a whole number.' };
    }
    if (f.min !== null && n < f.min) {
      return { error: `Must be at least ${f.min}.` };
    }
    if (f.max !== null && n > f.max) {
      return { error: `Must be at most ${f.max}.` };
    }
    return { value: n };
  }
  return { value: trimmed };
}

function placeholder(f: FormField): string | undefined {
  const d = f.default;
  if (d === null || d === undefined || typeof d === 'object') {
    return undefined;
  }
  return String(d);
}

function groupBySection(fields: FormField[]): { section: string | null; fields: FormField[] }[] {
  const groups: { section: string | null; fields: FormField[] }[] = [];
  for (const f of fields) {
    let g = groups.find((x) => x.section === f.section);
    if (!g) {
      g = { section: f.section, fields: [] };
      groups.push(g);
    }
    g.fields.push(f);
  }
  return groups;
}

function FieldControl({
  field: f,
  draft,
  invalid,
  onChange,
}: {
  field: FormField;
  draft: Draft;
  invalid: boolean;
  onChange: (value: Draft) => void;
}) {
  const id = `form-${f.key}`;
  const describedBy = [f.help ? `${id}-help` : '', invalid ? `${id}-error` : ''].filter(Boolean).join(' ') || undefined;
  if (f.kind === 'bool') {
    return (
      <Switch id={id} checked={draft === true} onCheckedChange={onChange} aria-describedby={describedBy} />
    );
  }
  if (f.kind === 'enum') {
    return (
      <select
        id={id}
        className={selectClass}
        value={String(draft)}
        aria-invalid={invalid || undefined}
        aria-describedby={describedBy}
        onChange={(e) => onChange(e.target.value)}
      >
        <option value="">{placeholder(f) ? `Default (${placeholder(f)})` : 'Not set'}</option>
        {f.choices.map((c) => (
          <option key={c} value={c}>
            {c}
          </option>
        ))}
      </select>
    );
  }
  if (f.kind === 'list') {
    return (
      <textarea
        id={id}
        rows={Math.min(8, Math.max(3, String(draft).split('\n').length + 1))}
        className={cn(selectClass, 'h-auto py-2 font-mono text-sm')}
        value={String(draft)}
        spellCheck={false}
        autoCapitalize="off"
        aria-invalid={invalid || undefined}
        aria-describedby={describedBy}
        onChange={(e) => onChange(e.target.value)}
      />
    );
  }
  const numeric = f.kind === 'int' || f.kind === 'float';
  return (
    <Input
      id={id}
      type={f.secret ? 'password' : 'text'}
      inputMode={numeric ? (f.kind === 'int' ? 'numeric' : 'decimal') : undefined}
      value={String(draft)}
      placeholder={placeholder(f)}
      autoCapitalize="off"
      autoComplete={f.secret ? 'new-password' : 'off'}
      spellCheck={false}
      aria-invalid={invalid || undefined}
      aria-describedby={describedBy}
      onChange={(e) => onChange(e.target.value)}
    />
  );
}

function FieldRow({
  field: f,
  draft,
  error,
  onChange,
}: {
  field: FormField;
  draft: Draft;
  error?: string;
  onChange: (value: Draft) => void;
}) {
  const id = `form-${f.key}`;
  if (f.kind === 'raw') {
    return (
      <div className="space-y-1.5">
        <div className="text-sm font-medium">{f.label}</div>
        <pre className="overflow-x-auto rounded-lg border border-input bg-muted p-3 font-mono text-xs leading-5">
          {f.raw}
        </pre>
        <p className="text-[13px] text-muted-foreground">Edit this one on the Raw tab.</p>
        {error ? <p className="text-[13px] text-destructive-foreground">{error}</p> : null}
      </div>
    );
  }
  const label = (
    <Label htmlFor={id} className={f.kind === 'bool' ? 'min-w-0 flex-1' : undefined}>
      {f.label}
    </Label>
  );
  const help = f.help ? (
    <p id={`${id}-help`} className="text-[13px] text-muted-foreground">
      {f.help}
    </p>
  ) : null;
  const err = error ? (
    <p id={`${id}-error`} className="text-[13px] text-destructive-foreground">
      {error}
    </p>
  ) : null;
  const control = <FieldControl field={f} draft={draft} invalid={Boolean(error)} onChange={onChange} />;
  if (f.kind === 'bool') {
    return (
      <div className="space-y-1">
        <div className="flex min-h-11 items-center justify-between gap-3">
          {label}
          {control}
        </div>
        {help}
        {err}
      </div>
    );
  }
  return (
    <div className="space-y-1.5">
      {label}
      {control}
      {help}
      {err}
    </div>
  );
}

export function ConfigForm({
  doc,
  onSubmit,
  saving,
  errors,
  allowEmpty = false,
}: {
  /** Saving with no edits is allowed: the file does not exist yet and the example is to be saved as it is. */
  allowEmpty?: boolean;
  doc: FormDoc;
  onSubmit: (edits: Edit[]) => void;
  saving: boolean;
  errors: Record<string, string>;
}) {
  const [drafts, setDrafts] = useState<Record<string, Draft>>({});
  const [local, setLocal] = useState<Record<string, string>>({});
  const [toggled, setToggled] = useState<Record<string, boolean>>({});

  // A fresh document (after a save and reload) starts clean.
  useEffect(() => {
    setDrafts({});
    setLocal({});
    setToggled({});
  }, [doc]);

  // Local mistakes win over what the server said last time about the same key.
  const shown = { ...errors, ...local };
  const known = new Set(doc.fields.map((f) => f.key));
  const stray = Object.entries(shown).filter(([k]) => !known.has(k));

  const editable = useMemo(() => doc.fields.filter((f) => f.kind !== 'raw'), [doc]);
  const changed = editable.filter((f) => f.key in drafts && drafts[f.key] !== initialDraft(f));
  const groups = useMemo(() => groupBySection(doc.fields), [doc]);

  function change(f: FormField, value: Draft) {
    setDrafts((d) => ({ ...d, [f.key]: value }));
    setLocal((l) => {
      if (!(f.key in l)) {
        return l;
      }
      const { [f.key]: _gone, ...rest } = l;
      return rest;
    });
  }

  function submit() {
    const edits: Edit[] = [];
    const problems: Record<string, string> = {};
    for (const f of changed) {
      const r = toValue(f, drafts[f.key]);
      if ('error' in r) {
        problems[f.key] = r.error;
      } else if (JSON.stringify(r.value) !== JSON.stringify(f.value ?? null)) {
        edits.push({ key: f.key, value: r.value });
      }
    }
    setLocal(problems);
    if (Object.keys(problems).length === 0 && (edits.length > 0 || allowEmpty)) {
      onSubmit(edits);
    }
  }

  function renderFields(fields: FormField[]) {
    return fields.map((f) => (
      <FieldRow
        key={f.key}
        field={f}
        draft={f.key in drafts ? drafts[f.key] : initialDraft(f)}
        error={shown[f.key]}
        onChange={(v) => change(f, v)}
      />
    ));
  }

  return (
    <div>
      <div className="space-y-4">
        {groups.map((g, i) => {
          const hasError = g.fields.some((f) => f.key in shown);
          const open = hasError || (g.section !== null && (toggled[g.section] ?? i === 0));
          return g.section === null ? (
            <div key="__none" className="space-y-4">
              {renderFields(g.fields)}
            </div>
          ) : (
            <Collapsible
              key={g.section}
              open={open}
              onOpenChange={(o) => setToggled((t) => ({ ...t, [g.section as string]: o }))}
              className="rounded-lg border border-border px-3">
              <CollapsibleTrigger className="min-h-11">{g.section}</CollapsibleTrigger>
              <CollapsibleContent className="space-y-4 pb-3">{renderFields(g.fields)}</CollapsibleContent>
            </Collapsible>
          );
        })}
      </div>
      {stray.length > 0 ? (
        <Alert variant="destructive" className="mt-4">
          <ul className="space-y-1">
            {stray.map(([k, m]) => (
              <li key={k}>
                {k}: {m}
              </li>
            ))}
          </ul>
        </Alert>
      ) : null}
      <Button className="mt-4 w-full" loading={saving} disabled={changed.length === 0 && !allowEmpty} onClick={submit}>
        Save and restart
      </Button>
    </div>
  );
}
