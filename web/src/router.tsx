import { createBrowserRouter } from 'react-router';
import { Shell } from './components/layout/Shell';
import { RecordingPage } from './pages/recording/RecordingPage';
import { ProfilingPage } from './pages/profiling/ProfilingPage';

export const router = createBrowserRouter(
  [
    {
      element: <Shell />,
      children: [
        { index: true, element: <RecordingPage /> },
        { path: 'profiling', element: <ProfilingPage /> },
      ],
    },
  ],
  { basename: import.meta.env.BASE_URL }
);
