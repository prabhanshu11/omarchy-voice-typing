import { Outlet } from 'react-router';
import { Nav } from './Nav';
import './Shell.css';

export function Shell() {
  return (
    <div className="app">
      <header className="app-header">
        <div className="header-left">
          <div className="logo">
            <span className="logo-icon">mic</span>
            <h1>voice-type</h1>
          </div>
          <Nav />
        </div>
      </header>
      <Outlet />
    </div>
  );
}
