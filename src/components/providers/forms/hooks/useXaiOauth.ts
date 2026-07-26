import { useManagedAuth } from "./useManagedAuth";
import { isWebRuntime } from "@/lib/runtime";

/** xAI OAuth device-code authentication hook. */
export function useXaiOauth() {
  return useManagedAuth("xai_oauth", undefined, !isWebRuntime());
}
