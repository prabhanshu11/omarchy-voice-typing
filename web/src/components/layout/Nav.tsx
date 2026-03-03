import { NavLink } from 'react-router';

export function Nav() {
  return (
    <nav className="nav">
      <NavLink to="/" end className={({ isActive }) => `nav-link ${isActive ? 'active' : ''}`}>
        Recordings
      </NavLink>
      <NavLink to="/profiling" className={({ isActive }) => `nav-link ${isActive ? 'active' : ''}`}>
        Profiling
      </NavLink>
    </nav>
  );
}
