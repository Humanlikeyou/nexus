import { resolve } from "node:path";

/** The one Ingress workflow export (live id eYdwlwZKTMV89z2J). Every test reads it from here. */
export const INGRESS_WORKFLOW_ID = "eYdwlwZKTMV89z2J";
export const INGRESS_WORKFLOW_REL = `stack/n8n/workflows/${INGRESS_WORKFLOW_ID}.json`;
export const INGRESS_WORKFLOW_PATH = resolve(__dirname, "../..", INGRESS_WORKFLOW_REL);
