import { type Project, ProjectSchema } from '@testing-ide/shared';
import { z } from 'zod';

import { invokeAndParse, invokeVoid } from './invoke';

const ProjectListSchema = z.array(ProjectSchema);

export async function createProject(name: string, rootPath: string): Promise<Project> {
  return invokeAndParse('create_project', ProjectSchema, { name, rootPath });
}

export async function listProjects(): Promise<Project[]> {
  return invokeAndParse('list_projects', ProjectListSchema);
}

export async function getProject(id: string): Promise<Project> {
  return invokeAndParse('get_project', ProjectSchema, { id });
}

export async function deleteProject(id: string): Promise<void> {
  return invokeVoid('delete_project', { id });
}
