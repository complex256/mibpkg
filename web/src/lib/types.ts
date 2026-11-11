// The JSON the car answers with (docs/reference/web-api.md). Views and components share these.

export interface Service {
  id: string;
  state: string;
  text?: string;
}

export interface ConfigFile {
  path: string;
  index: number;
  exists: boolean;
  conflict?: boolean;
}

export interface Permission {
  id: string;
  text: string;
  loud?: boolean;
  granted?: boolean;
}

export interface Installed {
  version: string;
  collection: string;
  previous: string | null;
  services: Service[];
  uses_scripts: boolean;
  missing_config: string[];
  last_result: string | null;
  configs: ConfigFile[];
  kind: string | null;
  permissions: Permission[];
  blocked: string[];
  previous_permissions: Permission[];
  grants_error: string | null;
}

export interface Offer {
  collection: string;
  origin: 'card' | 'online' | string;
  version: string;
  size?: number;
  compatible: boolean;
  kind?: string | null;
  permissions?: Permission[];
}

export interface MovedChoice {
  collection: string;
  version: string;
}

export interface Pkg {
  name: string;
  description?: string;
  installed: Installed | null;
  available: Offer[];
  update: string | null;
  update_from: string | null;
  moved_choices?: MovedChoice[];
}

export interface WifiSummary {
  configured: boolean;
  enabled: boolean;
  connected: boolean;
  active_ssid?: string | null;
}

export interface Status {
  version: string;
  unit: string;
  busy: boolean;
  job: string | null;
  updates: number;
  installed: number;
  disabled?: boolean;
  online?: string | null;
  supervisor: { running: boolean; services: number };
  last_result?: string | null;
  wifi?: WifiSummary | null;
}

export interface Job {
  id: string;
  label: string;
  package: string | null;
  lines: string[];
  next: number;
  truncated: boolean;
  done: boolean;
  ok: boolean;
  busy: boolean;
  result: string | null;
  lost?: boolean;
}

export interface ConfigDetail {
  path: string;
  exists: boolean;
  required: boolean;
  content: string;
  example: string | null;
  card: string | null;
}

export interface SourceRow {
  source: string;
  enabled: boolean;
  origin: string;
  collection: string | null;
  collection_name: string | null;
  packages: number;
  error: string | null;
}

export interface AvailableSource {
  name: string;
  description?: string;
  source: string;
  origin: string;
}

export interface SourceList {
  url: string | null;
  fetched_at: number | null;
  error: string | null;
  skipped: string[];
}

export interface SourcesReply {
  sources: SourceRow[];
  available: AvailableSource[];
  list: SourceList;
  card_skipped: string[];
}

export interface WebSettings {
  enabled: boolean;
  port: number;
  listen: string[];
  pin_set: boolean;
  urls: { class: string; url: string }[];
  interfaces_up: { name: string; address: string }[];
  interfaces: { hotspot?: string[] };
  warning?: string | null;
}

export interface WifiSettings {
  configured: boolean;
  enabled: boolean;
  connected: boolean;
  state: string;
  ssid: string;
  active_ssid: string | null;
  password_set: boolean;
  dhcp: boolean;
  ip: string;
  netmask: string;
  gateway: string;
  dns: string;
  ipv4: string | null;
  live_netmask: string | null;
  live_gateway: string | null;
  live_dns: string | null;
  error: string | null;
}

export interface UploadCandidate {
  collection: string;
  version: string;
  matches_file: boolean;
}

export interface UploadInspection {
  upload_id: string;
  name: string | null;
  version: string;
  description: string;
  size: number;
  sha256: string;
  kind: 'fresh' | 'upgrade' | 'same' | 'older';
  compatible: boolean;
  uses_scripts: boolean;
  services: string[];
  problems: string[];
  candidates: UploadCandidate[];
  preselected: string;
  ask: boolean;
  permissions: Permission[];
  needs_approval: string[];
  stays_blocked: string[];
  installed_collection: string | null;
  installed_version: string | null;
}

export interface LogInfo {
  id: string;
  label: string;
}
