import { type AnalysisOutcome, AnalysisOutcomeSchema } from '@testing-ide/shared';
import { z } from 'zod';

import { invokeAndParse } from './invoke';

export async function analyzeProject(projectId: string): Promise<AnalysisOutcome> {
  return invokeAndParse('analyze_project', AnalysisOutcomeSchema, { projectId });
}

export async function getAnalysisOutcome(projectId: string): Promise<AnalysisOutcome | null> {
  return invokeAndParse('get_analysis_outcome', z.nullable(AnalysisOutcomeSchema), { projectId });
}

