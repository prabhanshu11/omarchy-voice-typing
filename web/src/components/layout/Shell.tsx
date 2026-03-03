import { useEffect, useState } from 'react';
import { Outlet } from 'react-router';
import type { Stats } from '../../lib/types';
import { fetchStats } from '../../lib/api';
import { StatsBar } from './StatsBar';
import './Shell.css';

export function Shell() {
  const [stats, setStats] = useState<Stats | null>(null);

  useEffect(() => {
    fetchStats().then(setStats).catch(() => {});
  }, []);

  return (
    <div className="app">
      <header className="app-header">
        <div className="header-left">
          <div className="logo">
            <span className="logo-icon">mic</span>
            <h1>voice-type</h1>
          </div>
        </div>
        {stats && <StatsBar stats={stats} />}
      </header>
      <Outlet />
    </div>
  );
}
