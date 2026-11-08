// Hand-maintained mirror of Rust DTOs (crates/core, ts-rs).
// Regenerate via `pnpm types:gen` (cargo test --features ts-export) and re-mirror here.

export interface DetectedPackage {
  name: string;
  found: boolean;
  version: string | null;
  source: string;
}

export interface Profile {
  name: string;
  displayName: string;
  packages: string[];
}

export interface PackageSummary {
  name: string;
  category: string;
  wingetId: string | null;
}

export type InstallScope = "user" | "machine";

export interface PlanStep {
  packageId: string;
  dependsOn: string[];
  scope: InstallScope;
  state: StepState;
}

export type StepState =
  | "Pending"
  | "Resolving"
  | "Downloading"
  | "Installing"
  | "Verifying"
  | "Completed"
  | "Failed"
  | "Paused"
  | "RebootRequired"
  | "Skipped";

export interface InstallPlan {
  id: string;
  packageIds: string[];
  steps: PlanStep[];
  uacWarnings: number;
}

export interface StepEvent {
  planId: string;
  packageId: string;
  state: StepState;
  message: string | null;
  error: string | null;
}

export type DetectStatus =
  | "Missing"
  | { InstalledOk: { version: string | null } }
  | { VersionMismatch: { expected: string; actual: string } };
