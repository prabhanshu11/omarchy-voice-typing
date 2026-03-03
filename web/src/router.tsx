import { createBrowserRouter } from 'react-router';
import { Shell } from './components/layout/Shell';
import { ProfilingPage } from './pages/profiling/ProfilingPage';

export const router = createBrowserRouter([
  {
    element: <Shell />,
    children: [
      { index: true, element: <ProfilingPage /> },
    ],
  },
]);
