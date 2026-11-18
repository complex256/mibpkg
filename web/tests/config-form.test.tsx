import { beforeEach, describe, expect, test, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { ConfigForm } from '../src/components/config-form';
import { ConfigView } from '../src/views/config';
import { mockApi, reply, resetUi } from './helpers';
import type { ConfigDetail, FormDoc, FormField } from '../src/lib/types';

beforeEach(resetUi);

function field(extra: Partial<FormField>): FormField {
  return {
    key: 'k',
    label: 'K',
    help: '',
    kind: 'string',
    source: 'schema',
    value: null,
    default: null,
    choices: [],
    min: null,
    max: null,
    secret: false,
    section: null,
    raw: null,
    ...extra,
  };
}

function doc(fields: FormField[]): FormDoc {
  return { mode: 'schema', fields, reason: null };
}

function detail(extra: Partial<ConfigDetail> = {}): ConfigDetail {
  return {
    path: 'carmqtt.toml',
    exists: true,
    required: true,
    content: 'host = "a"\n',
    example: null,
    card: null,
    form: doc([field({ key: 'host', label: 'Host', value: 'a' })]),
    ...extra,
  };
}

function serve(cfg: ConfigDetail, extra: Record<string, (init?: RequestInit) => Promise<Response>> = {}) {
  mockApi({ 'GET /api/packages/carmqtt/config/0': reply(200, cfg), ...extra });
}

describe('the config form', () => {
  test('renders_switch_input_and_select_for_kinds', () => {
    render(
      <ConfigForm
        saving={false}
        errors={{}}
        onSubmit={() => {}}
        doc={doc([
          field({ key: 'on', label: 'Enabled', kind: 'bool', value: true }),
          field({ key: 'host', label: 'Host', help: 'Broker name', value: 'a' }),
          field({ key: 'mode', label: 'Mode', kind: 'enum', choices: ['fast', 'slow'], value: 'fast' }),
        ])}
      />,
    );
    expect(screen.getByRole('switch', { name: 'Enabled' })).toBeChecked();
    expect(screen.getByLabelText('Host')).toHaveValue('a');
    expect(screen.getByText('Broker name')).toBeInTheDocument();
    expect(screen.getByRole('combobox', { name: 'Mode' })).toHaveValue('fast');
  });

  test('submits_only_changed_fields', async () => {
    const onSubmit = vi.fn();
    render(
      <ConfigForm
        saving={false}
        errors={{}}
        onSubmit={onSubmit}
        doc={doc([
          field({ key: 'host', label: 'Host', value: 'a' }),
          field({ key: 'port', label: 'Port', kind: 'int', value: 1883 }),
        ])}
      />,
    );
    expect(screen.getByRole('button', { name: /Save and restart/ })).toBeDisabled();
    await userEvent.clear(screen.getByLabelText('Port'));
    await userEvent.type(screen.getByLabelText('Port'), '8883');
    await userEvent.click(screen.getByRole('button', { name: /Save and restart/ }));
    expect(onSubmit).toHaveBeenCalledWith([{ key: 'port', value: 8883 }]);
  });

  test('empty_optional_field_sends_null', async () => {
    const onSubmit = vi.fn();
    render(
      <ConfigForm
        saving={false}
        errors={{}}
        onSubmit={onSubmit}
        doc={doc([field({ key: 'user', label: 'User', value: 'bob' })])}
      />,
    );
    await userEvent.clear(screen.getByLabelText('User'));
    await userEvent.click(screen.getByRole('button', { name: /Save and restart/ }));
    expect(onSubmit).toHaveBeenCalledWith([{ key: 'user', value: null }]);
  });

  test('an invalid number is a field error and is not sent', async () => {
    const onSubmit = vi.fn();
    render(
      <ConfigForm
        saving={false}
        errors={{}}
        onSubmit={onSubmit}
        doc={doc([field({ key: 'port', label: 'Port', kind: 'int', value: 1 })])}
      />,
    );
    await userEvent.clear(screen.getByLabelText('Port'));
    await userEvent.type(screen.getByLabelText('Port'), 'abc');
    await userEvent.click(screen.getByRole('button', { name: /Save and restart/ }));
    expect(onSubmit).not.toHaveBeenCalled();
    expect(screen.getByText(/whole number/)).toBeInTheDocument();
  });

  test('shows_field_error_from_400', () => {
    render(
      <ConfigForm
        saving={false}
        errors={{ port: 'Port must be at most 65535' }}
        onSubmit={() => {}}
        doc={doc([field({ key: 'port', label: 'Port', kind: 'int', value: 1 })])}
      />,
    );
    expect(screen.getByText('Port must be at most 65535')).toBeInTheDocument();
  });

  test('secret_field_is_password_input', () => {
    render(
      <ConfigForm
        saving={false}
        errors={{}}
        onSubmit={() => {}}
        doc={doc([field({ key: 'pw', label: 'Password', secret: true, value: 'x' })])}
      />,
    );
    expect(screen.getByLabelText('Password')).toHaveAttribute('type', 'password');
  });

  test('raw fields are read-only and never submitted', async () => {
    const onSubmit = vi.fn();
    render(
      <ConfigForm
        saving={false}
        errors={{}}
        onSubmit={onSubmit}
        doc={doc([
          field({ key: 't', label: 'Table', kind: 'raw', raw: 'a = 1' }),
          field({ key: 'host', label: 'Host', value: 'a' }),
        ])}
      />,
    );
    expect(screen.getByText('a = 1')).toBeInTheDocument();
    await userEvent.type(screen.getByLabelText('Host'), 'b');
    await userEvent.click(screen.getByRole('button', { name: /Save and restart/ }));
    expect(onSubmit).toHaveBeenCalledWith([{ key: 'host', value: 'ab' }]);
  });

  test('a list is one item per line and numbers stay numbers', async () => {
    const onSubmit = vi.fn();
    render(
      <ConfigForm
        saving={false}
        errors={{}}
        onSubmit={onSubmit}
        doc={doc([field({ key: 'ids', label: 'Ids', kind: 'list', value: [1, 2] })])}
      />,
    );
    expect(screen.getByLabelText('Ids')).toHaveValue('1\n2');
    await userEvent.type(screen.getByLabelText('Ids'), '\n3\n');
    await userEvent.click(screen.getByRole('button', { name: /Save and restart/ }));
    expect(onSubmit).toHaveBeenCalledWith([{ key: 'ids', value: [1, 2, 3] }]);
  });
});

describe('the config view', () => {
  test('raw_tab_shows_textarea_and_round_trips', async () => {
    const bodies: unknown[] = [];
    serve(detail(), {
      'PUT /api/packages/carmqtt/config/0': async (init) => {
        bodies.push(init?.body);
        return new Response(JSON.stringify({ job: 'j1' }), { status: 202 });
      },
    });
    render(<ConfigView name="carmqtt" index="0" />);
    await userEvent.click(await screen.findByRole('button', { name: 'Raw' }));
    const area = screen.getByLabelText(/carmqtt · carmqtt\.toml/);
    expect(area).toHaveValue('host = "a"\n');
    await userEvent.type(area, '# x');
    await userEvent.click(screen.getByRole('button', { name: /Save and restart/ }));
    expect(bodies).toEqual(['host = "a"\n# x']);
  });

  test('form save posts only edits and maps a 400 onto the field', async () => {
    const bodies: unknown[] = [];
    serve(detail(), {
      'POST /api/packages/carmqtt/config/0/form': async (init) => {
        bodies.push(JSON.parse(String(init?.body)));
        return new Response(JSON.stringify({ error: 'Fix it', fields: [{ key: 'host', message: 'Host is bad' }] }), {
          status: 400,
        });
      },
    });
    render(<ConfigView name="carmqtt" index="0" />);
    await userEvent.type(await screen.findByLabelText('Host'), 'b');
    await userEvent.click(screen.getByRole('button', { name: /Save and restart/ }));
    expect(await screen.findByText('Host is bad')).toBeInTheDocument();
    expect(bodies).toEqual([{ edits: [{ key: 'host', value: 'ab' }] }]);
  });

  test('no_form_tab_when_form_is_null_or_raw_mode', async () => {
    serve(detail({ form: { mode: 'raw', fields: [], reason: 'Could not parse line 3' } }));
    render(<ConfigView name="carmqtt" index="0" />);
    expect(await screen.findByText(/Could not parse line 3/)).toBeInTheDocument();
    expect(screen.getByLabelText(/carmqtt · carmqtt\.toml/)).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Form' })).not.toBeInTheDocument();
  });

  test('no tab when the file is not toml', async () => {
    serve(detail({ form: null }));
    render(<ConfigView name="carmqtt" index="0" />);
    expect(await screen.findByLabelText(/carmqtt · carmqtt\.toml/)).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Form' })).not.toBeInTheDocument();
  });

  test('no_form_tab_during_card_conflict', async () => {
    serve(detail({ card: 'host = "b"\n' }));
    render(<ConfigView name="carmqtt" index="0" />);
    expect(await screen.findByText(/changed both on the SD card/)).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Form' })).not.toBeInTheDocument();
    expect(screen.getByLabelText(/carmqtt · carmqtt\.toml/)).toBeInTheDocument();
  });
});
