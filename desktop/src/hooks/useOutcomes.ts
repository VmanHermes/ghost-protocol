import { useCallback, useEffect, useState } from "react";
import { listOutcomes } from "../api";
import type { OutcomeRecord } from "../types";

export type UseOutcomesOptions = {
  daemonUrl: string;
  limit?: number;
  pollInterval?: number;
};

export function useOutcomes({
  daemonUrl,
  limit = 10,
  pollInterval = 10_000,
}: UseOutcomesOptions): OutcomeRecord[] {
  const [outcomes, setOutcomes] = useState<OutcomeRecord[]>([]);

  const fetch = useCallback(async () => {
    try {
      const data = await listOutcomes(daemonUrl, limit);
      setOutcomes(data);
    } catch {
      // Silently ignore fetch errors
    }
  }, [daemonUrl, limit]);

  useEffect(() => {
    void fetch();
    const timer = setInterval(() => void fetch(), pollInterval);
    return () => clearInterval(timer);
  }, [fetch, pollInterval]);

  return outcomes;
}
