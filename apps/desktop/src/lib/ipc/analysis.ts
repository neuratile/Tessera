import { type AnalysisOutcome, AnalysisOutcomeSchema } from '@testing-ide/shared';

import { invokeAndParse } from './invoke';

export async function analyzeProject(projectId: string): Promise<AnalysisOutcome> {
  return invokeAndParse('analyze_project', AnalysisOutcomeSchema, { projectId });
}
