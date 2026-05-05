import { type HealthStatus, HealthStatusSchema } from '@testing-ide/shared';

import { invokeAndParse } from './invoke';

export async function healthCheck(): Promise<HealthStatus> {
  return invokeAndParse('health_check', HealthStatusSchema);
}
