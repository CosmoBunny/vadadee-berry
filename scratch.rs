pub fn brush_stroke_outline(points: &[([f64; 2], f32)]) -> kurbo::BezPath {
    let mut path = kurbo::BezPath::new();
    let n = points.len();
    if n == 0 {
        return path;
    }
    if n == 1 {
        let (p, w) = points[0];
        let r = w as f64 / 2.0;
        return kurbo::Circle::new(kurbo::Point::new(p[0], p[1]), r).to_path(0.1);
    }
    
    let mut lefts = Vec::with_capacity(n);
    let mut rights = Vec::with_capacity(n);
    
    for i in 0..n {
        let (p, w) = points[i];
        let r = w as f64 / 2.0;
        
        let mut t_x = 0.0;
        let mut t_y = 0.0;
        
        if i == 0 {
            t_x = points[1].0[0] - p[0];
            t_y = points[1].0[1] - p[1];
        } else if i == n - 1 {
            t_x = p[0] - points[i - 1].0[0];
            t_y = p[1] - points[i - 1].0[1];
        } else {
            t_x = points[i + 1].0[0] - points[i - 1].0[0];
            t_y = points[i + 1].0[1] - points[i - 1].0[1];
        }
        
        let len = t_x.hypot(t_y);
        if len > 1e-5 {
            t_x /= len;
            t_y /= len;
        } else {
            t_x = 1.0;
            t_y = 0.0;
        }
        
        let nx = -t_y;
        let ny = t_x;
        
        lefts.push(kurbo::Point::new(p[0] + nx * r, p[1] + ny * r));
        rights.push(kurbo::Point::new(p[0] - nx * r, p[1] - ny * r));
    }
    
    path.move_to(lefts[0]);
    for i in 1..n {
        path.line_to(lefts[i]);
    }
    
    // Add end cap
    let end_p = kurbo::Point::new(points[n-1].0[0], points[n-1].0[1]);
    let end_r = points[n-1].1 as f64 / 2.0;
    // We could add an arc here, but a simple line or two is okay for now, or use kurbo arc.
    // For kurbo arc, we need the start and sweep angle.
    path.line_to(rights[n-1]);
    
    for i in (0..n-1).rev() {
        path.line_to(rights[i]);
    }
    
    // Start cap
    path.close_path();
    
    path
}
