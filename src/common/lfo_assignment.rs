use crate::common::{ClapParamId, param_store::ParamStore};

#[derive(Debug, Clone, Copy)]
pub struct ModRouteParamIds<P> {
    pub source: P,
    pub target: P,
    pub depth: P,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct LfoAssignmentState {
    armed_lfo: Option<usize>,
}

impl LfoAssignmentState {
    pub fn armed_lfo(&self) -> Option<usize> {
        self.armed_lfo
    }

    pub fn toggle(&mut self, lfo_index: usize) {
        self.armed_lfo = if self.armed_lfo == Some(lfo_index) {
            None
        } else {
            Some(lfo_index)
        };
    }
}

#[derive(Debug, Clone, Copy)]
pub struct LfoAssignmentConfig<'a, P> {
    pub routes: &'a [ModRouteParamIds<P>],
    pub first_lfo_source: u8,
    pub lfo_count: u8,
    pub default_depth: f32,
}

impl<P> LfoAssignmentConfig<'_, P>
where
    P: ClapParamId,
{
    pub fn lfo_source_value(&self, lfo_index: usize) -> u8 {
        self.first_lfo_source + lfo_index.min(self.lfo_count.saturating_sub(1) as usize) as u8
    }

    pub fn has_assignment(&self, params: &ParamStore<P>, target: u8) -> bool {
        self.routes.iter().any(|route| {
            let source = params.get(route.source) as u8;
            source >= self.first_lfo_source
                && source < self.first_lfo_source.saturating_add(self.lfo_count)
                && params.get(route.target) as u8 == target
                && params.get(route.depth).abs() > 0.001
        })
    }

    pub fn has_lfo_assignment(&self, params: &ParamStore<P>, lfo_index: usize, target: u8) -> bool {
        let source = self.lfo_source_value(lfo_index);
        self.routes.iter().any(|route| {
            params.get(route.source) as u8 == source
                && params.get(route.target) as u8 == target
                && params.get(route.depth).abs() > 0.001
        })
    }

    pub fn assign(
        &self,
        params: &ParamStore<P>,
        lfo_index: usize,
        target: u8,
        mut set_param_once: impl FnMut(P, f32),
    ) {
        let source = self.lfo_source_value(lfo_index);
        let route_index = self
            .routes
            .iter()
            .position(|route| {
                params.get(route.source) as u8 == source && params.get(route.target) as u8 == target
            })
            .or_else(|| {
                self.routes
                    .iter()
                    .position(|route| params.get(route.depth).abs() <= 0.001)
            })
            .or_else(|| {
                self.routes
                    .iter()
                    .position(|route| params.get(route.target) as u8 == target)
            })
            .unwrap_or(0);

        let route = self.routes[route_index];
        set_param_once(route.source, f32::from(source));
        set_param_once(route.target, f32::from(target));
        if params.get(route.depth).abs() <= 0.001 {
            set_param_once(route.depth, self.default_depth);
        }
    }
}
